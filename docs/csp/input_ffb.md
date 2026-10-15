# Input and force feedback

CSP changes how acs.exe reads wheels, gamepads, keyboard and mouse, and what it sends back to the hardware. It
(a) wraps or replaces the DirectInput / XInput device classes of `ksDI.lib`, `X360CarControl.obj`,
`KeyboardCarControl.obj`, `DICarControl.obj` and `MouseHider.obj`; (b) post-processes the steering force in
`Car::getSteerFF` and in the `sendFF` chain; (c) adds a second binding layer (the `__EXT_*` / `__CM_*` sections of
`controls.ini`) and the Lua / Python API on top; (d) ships device-specific code of its own: DualSense / DualShock /
Joy-Con, Logitech SDK, TrackIR, G27 LEDs, Wooting. On the owner's PC (Xbox One pad, no wheel, no module ini
overrides) almost all of it is idle; what is active is listed in section 5.

Conventions: CSP addresses are `dwrite.dll RVA 0x...` (image base 0x180000000); acs.exe addresses are `0x14...`.
"Cat" is the category of `physics_car_hooks.md` 2: A = pass-through / fires CSP events, B = wraps the original
(pre/post work), C = replaces it (original not called, or only conditionally), D = lookup (CSP only calls it).

## 1. What it does

### 1.1 Device layer (DirectInput) `[dll][log-csp]`

* **Start-up watchdog.** `DirectInput::DirectInput` is wrapped by a re-entrancy guard; it logs "Starting to initialize
  DirectInput devices" / "DirectInput devices are ready" and starts a thread "CSP: validating DirectInput
  initialization" (function 0x5ae520, 148 B, uses `Sleep`) that prints "DirectInput devices take unusually long to
  initialize." and "DirectInput initialization: timeout exceeded" when a USB driver hangs. The owner's log shows the
  thread and a 0.65 s enumeration (27,936 -> 28,587 ms).
* **Enumeration.** CSP's own callback (0x5af860, 1898 B) logs every device ("New joystick found: ...", product GUID,
  instance GUID, usage page / usage, "Device type: GAMEPAD STANDARD") and knows Sony product GUIDs
  `0CE6054C-...` (DualSense) and `0DF2054C-...` (DualSense Edge). A table of DI device sub-type names (0x5ae870,
  4,060 B: `DRIVING COMBINEDPEDALS`, `DRIVING_LIMITED`, `GAMEPAD STANDARD`, `GAMEPAD TILT`, `JOYSTICK STANDARD`,
  `SUPPLEMENTAL SHIFTER`, ...) is used to rank devices ("A better device is here: ...").
* **Device order fix.** After the original `initJoys` CSP runs `fix_devices_order` (0x5b1df0, 6,082 B): it matches the
  connected devices with the `[CONTROLLERS]` section of `controls.ini` (`CON<n>` name, `PGUID<n>` product GUID,
  `__IGUID<n>` instance GUID; found "by I.", "by N.", "by P.") and re-sorts the device vector so that `JOY=<n>` indices
  of the bindings stay valid when USB ports change; missing devices get a **placeholder** InputDevice named
  `<placeholder>` (13 characters, wide string at 0x221d230). Hidden key `[CONTROL] FIX_ORDER2` (shipped commented out,
  default on). The owner's log: "fix_devices_order(): D:1, I:1, M:0 ... Controls order does not need fixing".
* **Hot-plug.** Thread `csp_devices_monitor` (0x5b0370, 429 B) and "CSP: controllers rescan" (0x5b1bb0): with
  `[CONTROL] LOOK_OUT_FOR_MISSING=1` a device that appears during the race and "looks like a better fit" replaces the
  current one ("%s is connected", "Failed to create newly connected device, trying again in a bit"). Default is 0.
* **Own InputDevice constructor.** `InputDevice::InputDevice` is replaced (0x5fb330 -> real body 0x5fafc0, 878 B):
  create the DI device, set range property, set cooperative level, set data format, log each step with the ID; on
  failure the object becomes a zeroed 0x100-byte placeholder. Cooperative level and data format of the original were
  not read (open question 1).
* **Poll.** `DirectInput::poll` (0x5ae5c0) is replaced: it loops the devices itself. `InputDevice::poll` (hook 0x5fc060,
  thunk to 0x5fc070) is replaced: `Poll`, then `GetDeviceState` of a 272-byte `DIJOYSTATE2`; on "input lost" /
  "not acquired" it re-`Acquire`s; axes are copied as raw integers or, for "normalized" devices, multiplied by 0.0001;
  a per-device failure counter (byte at +0xcb) stops polling a vanished device after 128 failures and retries once
  per 128 polls; the first good read after more than 120 failures logs "%s is reconnected" and raises a CSP
  notification keyed by a CRC32 of the device name. With **async mode** (see 1.3) the steering device is polled by a
  background thread and the physics thread only copies the last snapshot (0xA8 bytes at RVA 0x26a4c38, spin-lock
  byte 0x26a132c, device pointer 0x26a4c20, mode flag 0x26a132d) [dll][interp].
* **Reconnect.** `reconnectInputDevices` (0xe215e9, 677 B; name at 0x2273378) and the Lua plug-in `reconnect_devices`
  (`small_tweaks.ini [BASIC] RECONNECT_WHEEL_ON_APP_RESTORE`, default 0: "might help with strange Logitech
  behavior") re-create the DI devices after AC is restored from background; strings "Reconnecting: ",
  "FFB is reinitialized" (0x5fb530, 1,726 B).
* **Shutdown.** `DirectInput::~DirectInput` sets a global "input is shutting down" byte (RVA 0x268a7af, which
  `DICommandManager::step` tests) and, in CSP mode, destroys the devices itself, skipping placeholders.

### 1.2 FFB output path `[dll]`

Physics side (per 3 ms step, player car only, car index 0): `Car::getSteerFF` hook (0x595800) -> original; then the
CSP per-car FFB object (`ext+0x268`) and the script value; then CSP event bus 0x268ff60 (handlers get `{car, &ff}`);
NaN / Inf -> 0. Details of that hook are in `physics_car_hooks.md` (row `Car::getSteerFF`). Hardware side:

1. `DICarControl::acquireControls` hook (0x1097490): original in the middle, CSP pre / post work (steering
   extrapolation 0xecf070 / 0xecf1a0, see 1.6) and CSP event bus 0x2695f50.
2. `DICarControl::setVibrations` hook (0x1097900, **C**): re-implements the mix of the five vibration channels;
   with the global byte at RVA 0x26a6ee4 set (written by the `ffb_tweaks` loader right after reading `CLEAN_FFB`,
   so it **is** CLEAN_FFB `[dll]`) and an extended-physics car present, the 5th channel (index 4, the slip /
   ABS contribution weighted by +0xa74) is dropped: "remove unnecessary FFB post-processing from cars with custom
   physics to prevent doubling".
3. `DICarControl::sendFF` hook (0x1097880): adds a per-car additive offset (CSP record +0x1c; `physics.offsetFinalFFB`
   in Lua) to the force; with `FF_SKIP_STEPS > 0` and the byte at 0x269522b set (AVERAGE_SKIPPED_FFB, default 1
   `[interp]`) it accumulates the skipped steps and sends their **average** on the step that goes out; then the
   original.
4. `InputDevice::sendFF` hook (0x5fc2b0): returns early when the device is absent; when shutting down, or when a
   sim flag (`[sim+0x1b8]+0x3fc`, "FFB disabled", cf. `FADE_IN_AFTER_LOADING2` [interp]) is set, sends **force 0 /
   damper -1** once; otherwise runs the handler list at RVA 0x2690c88 (a handler can consume the call, this is where
   the FFB post-processing script and custom soft lock live [interp]) and then CSP's own submit (0x5fce20, with
   profiler scope "Sending FFB").
5. **Adaptive FFB** (`[TWEAKS] ADAPTIVE_FFB`, default 0): when sending takes too long the log shows "FFB is taking
   too long, increasing FFB frames gap to N" (mode 1) or "FFB is taking too long, switching to async processing"
   (mode 2); async submit / poll run in the thread "CSP: background FFB" ("Async submitting FFB", "Async polling
   input device"; 0x5fc720, 932 B, 0x5fcae0, 830 B). The physics profiler also prints "FFB is being sent while
   computing the first car" (0x686912) [dll].
6. **Custom FFB init** (`[OTHER_EXPERIMENTS] CUSTOM_FFB_INIT`, default 0, flag RVA 0x268a8ce written in the
   `general.ini` loader 0x138ded0): `InputDevice::initFF` hook (0x5fab00) runs the handler list 0x2690c80, then the
   original (flag 0) or CSP's own version (flag 1): `SetProperty(DIPROP_AUTOCENTER = 0)`, then
   `CreateEffect` of a one-axis infinite constant force and of a one-axis infinite damper (both gain 10000), with
   the log lines "[InputDevice.initFF] Force effect is ready / Damping effect is ready / Failed to ...".
   The effects are the same two that acs.exe creates (`docs/atlas/input_ffb.md`); CSP does not add spring, friction
   or periodic effects.
7. `InputDevice::setG27Leds` hook (0x5fc1a0): handler list 0x2690c78 may rewrite the three RPM thresholds or cancel
   the call; then the original (DirectInput escape for the Logitech LEDs).

### 1.3 FFB modules of `ffb_tweaks.ini` (loader function 0xcb87a0, 5,620 B, not decompiled)

* **Custom soft lock** (`[CUSTOM_SOFT_LOCK]`, on): a smoother replacement of AC's lock; the shipped Lua example
  `lua/ffb-postprocess/soft-lock-test/ffb.lua` says its logic "matches the custom in CSP": inside a padding zone of
  `PADDING` degrees at the end of the steering range the force is blended toward a push back to centre scaled by
  `FORCE_FACTOR`, plus a term from the wheel's angular speed scaled by `SPEED_FACTOR`, with an optional damper.
  `ac.disableSoftLock()` switches the original off for scripts.
* **Range compression** (`RANGE_COMPRESSION`, 1.0 = off) and `RANGE_COMPRESSION_ASSIST` (0): when on, the byte at
  RVA 0x269958a is set and `Car::getSteerFF` forces `steerAssist` (Car+0x3c70) to 1.0 while calling the original, so
  that the car's `STEER_ASSIST` power law is turned into compression later `[dll][interp]`.
* **Dirt vibration** (`DIRT_VIBRATION` 0..1.5, hidden `_FREQUENCY`, `_VARIATION`, `_REMAP`), **gyro 2**
  (`[GYRO2] ENABLED`, `ORIGINAL_GYRO`, `OLD_VERSION`, hidden `STRENGTH`; "doesn't affect cars using extended
  physics"), **real feel** steering (`[REAL_FEEL] ENABLED`, `WHEEL_MAX_NM` 2..35, `ENABLE_FOR_ALL`) and real-feel
  brake pedal (`[REAL_FEEL_BRAKE]`: `BRAKE_MAX_KG`, `BRAKE_MAX_INACTIVE_PERC`, `CLAMP_FORCES`) for cars that carry
  the physics code (needs extended physics, see `physics_documented.md`).
* **FFB post-processing script** (`[POSTPROCESSING_SCRIPT] ENABLED=0`, `IMPLEMENTATION=alternative`): loads
  `extension/lua/ffb-postprocess/<name>/ffb.lua` (loader 0x415870, 670 B); entry
  `script.update(ffbValue, ffbDamper, steerInput, steerInputSpeed, dt) -> value, damper` at physics rate, with the
  globals `car` and `sim`. Shipped: `alternative` (an experimental complete re-implementation of FFB from side
  force, front force, suspension travel and damping, 7,420 B, MIT licence header, settings `sideForceK 0.3`,
  `frontForceK 0.4`, `suspTravelK 0.15`, `dampK 0.07`, `baseForceK 1`) and `soft-lock-test`. The script may call
  `ac.setPureFFBMultiplier`, `ac.unlockFFBLimits` ("it's possible to break someone's fingers" with Direct Drive),
  `ac.disableLowSpeedFFBReduction` (registration 0x4154a0), `ac.getFFBGain`, `ac.getCarPhysicsRate`.
* **FFB hotkeys:** `__EXT_FFB_INCREASE / _DECREASE / _RESET` with `small_tweaks.ini [FFB_ADJUSTMENTS] STEP=0.05`,
  `SHOW_MESSAGE=1`; they call the lookup `CarAvatar::modifyUserFFGain` (0x1400d90f0).
* **AC's own FFB app** `FFBForm::FFBForm` is replaced by the stub 0x305980 (returns 0, shared with other apps) so
  that the stock "Force Feedback Controller" form is not created; the hint string says "To tune FFB live, try
  Controller Tweaks app from the App Shelf" (not installed on this PC) `[dll][interp]`.

### 1.4 Wheel provider and bindings `[dll]`

* `DICarControl::DICarControl` hook (0x1096dc0, 329 B): original, then CSP creates a per-provider record (vector
  at RVA 0x26aaf18, index = byte at provider+0xa85, builder 0x1097d10 which reads `[GEARUP` / `[GEARDN`
  names), copies one byte of the record into the device object (+0xca) and registers the provider in the car's
  map (record +0x478). `~DICarControl`
  (0x1096f10) clears the singleton pointer (RVA 0x26aaf10) and tail-calls the original.
* `DICarControl::getAction` (0x1097740, 314 B): global "no input" byte (RVA 0x2695016) -> false; per-action forced
  flags (table at RVA 0x26a9d28) OR the original; then the actions: **0 / 1 (glance left / right)** are also true
  when an analog "look" value (`__EXT_LOOK_AXIS`, `ac.getJoystickLook`) is below 0.33 / above 0.67; **6 (horn)**
  turns into a toggle for cars whose model object flags it (`[[car+0x28]+0x440]+0x11b`) `[interp]`. The same horn
  toggle is in the Joypad and Keyboard `getAction`.
* **Extended binding cells.** `DIControlButton::load` (0x5a7490, 2,916 B) runs the original, then reads the extras
  from the same `controls.ini` section: `ENABLED`, `KEY_MODIFICATOR` (a keyboard modifier name, stored as a key
  code), `__CM_POV` / `POV` (hat index 0..3) and `__CM_POV_DIR` / `POV_DIR` (-1..3: the hat direction is used as a
  button; Content Manager writes the `__CM_` variants), and **alternative bindings**: every section named
  `__ALT_<SECTION>_<n>` (the prefix `__CM_ALT_` exists as well) becomes a child `DIControlButton`; bit 0x40 of
  the cell marks "has alternatives" and `getValue` / `getKeyboardValue` OR them (hooks 0x5a8000, 0x5a8130). A
  global byte (RVA 0x2695be2) blanks all bindings while a CSP UI is capturing input.
* `DICommandManager::step` (0x5a7260, 60 B, **C**): turns the D-pad of the steering device into window key
  presses as AC does, but only when the global flags (RVA 0x268a7af shut-down, RVA 0x26a2c47 "CSP UI owns the pad")
  are clear, and the message goes with `lParam = 0x10000000`; the key event is also queued on the main thread.
* **H-shifter toggle:** `__EXT_SWITCH_HSHIFTER` "Toggle H-shifter (works with wheel input mode in races with
  cars with H-shifter)" `[ini]`; implemented in Lua / not mapped.

### 1.5 Gamepad (X360) `[dll][ini]`

* `X360Joypad::acquire` (0x6bf270, **C**): `XInputGetState(0)` through the lookup slot (RVA 0x268b848, **not** a hook:
  `XInputGetState` and `XInputSetState` are both lookups, `attach.md` 1.8 says one hook); the button mask is OR-ed with
  a second mask (RVA 0x26a85c8, buttons injected by `ac.setXbox`, pad-as-mouse and scripts) and, while the CSP UI
  owns the D-pad (flag 0x26a2c47), the four D-pad bits (0xfff0 mask) are removed; a failed read clears the state
  only if the pad never answered before.
* `X360Joypad::setVibrations` (0x6bf260, **C**): clamps both motors to 0..1, scales to 0..65535 and calls
  `XInputSetState(0)` (RVA 0x268b850) inside a thread-local re-entrancy mark that the KERNEL32 `DeviceIoControl`
  hook (0xf81310, 737 B, string "Failed to send trigger vibrations to the device", imports `WriteFile`) reads:
  CSP intercepts the XUSB IOCTL and adds **Xbox One impulse-trigger rumble** [interp].
* `JoypadCarControl` (**ctor C** 0x601b50 -> 0x601b80 961 B; **acquireControls B** 0x602b60; **getAction B**
  0x602d10; **sendFF C** 0x602eb0; **setVibrations C** 0x602f10; **dtor B** 0x602430 which keeps usage counters per
  gamepad type 0..11):
  * acquireControls: runs the original with the CSP per-gamepad record (RVA 0x25e1660, index = byte at provider+0x39)
    and, unless the record's pair at +0x90 is (1, 0), with a parameter-override frame pushed on the provider
    (pointer at +0x40); then **event bus 0x2690c98** gets `{provider, controls, steer lock, dt}` = where the Lua
    **joypad assist** script runs; output steer (+0x2c of CarControls) is taken from the record.
  * sendFF: decimation counter; the left / right motor values are sent through the X360Joypad vtable (+0x30) every
    **10** steps (33 Hz), or every **5** steps (66 Hz) when `gamepad_fx.ini [BASIC] BOOST_FREQUENCY=1` (flag RVA
    0x268a8cf, written in the gamepad_fx loader 0xd0f480).
  * setVibrations: all five inputs zero -> both motors 0 and bit 0 of byte +0x4a set (skip sending); otherwise left
    motor = `RUMBLE_INTENSITY` while the kerb level > 0.05, else (legacy flag, +0x38) engine level * RUMBLE * 0.15;
    right motor = |slip level| * RUMBLE; the left value is floored at FLT_MIN so that "non-zero" is preserved. The
    values match `docs/atlas/input_ffb.md` for AC's own mix; CSP adds the skip bit and the rate.
  * Reads from `controls.ini [X360]`: `JOYPAD_INDEX` (new, selects the pad; owner has 0), `AXIS_REMAP_THROTTLE`,
    `AXIS_REMAP_BRAKES` (strings 0x221d848 / 0x221d860; semantics not mapped).
* `SecondaryJoypadButton::isPressed` (0x69d7d0, **C**): a button is pressed if the mask says so, or the optional
  key mapped to it is down in CSP's keyboard-state table (512 B per frame at RVA 0x26a8660, double-buffered by
  `[0x26a8e80]`), unless a CSP UI is capturing the keyboard.
* **Gamepad scripts** (`gamepad_fx.ini [JOYPAD_ASSIST] ENABLED=0`): `extension/lua/joypad-assist/<name>/assist.lua`,
  entry `script.update(dt)` on the physics thread with the globals `car`, `sim`, `__carIndex`, `__gamepadIndex`;
  the script reads and writes the mutable `ac.getJoypadState()` (`ac.StateJoypadData`: 53 fields, inputs such as
  `steerStickX`, `ffb`, `mzForce`, `gForces`, `ndSlipL/R/RL/RR`, `surfaceVibrationGain/Length`, `speedKmh`, and
  outputs `steer`, `gas`, `brake`, `clutch`, `handbrake`, the 20 button booleans, `requestedGearIndex`, `mguhMode`,
  `vibrationLeft`, `vibrationRight`). Shipped: **Gamepad Force** (default implementation: steering as a virtual
  mass-spring driven by stick, FFB and speed, Race and Drift modes, `MODE_SWITCH_BUTTON` default 128 = right thumb),
  **Gyrosteer** (DualSense / DualShock gyroscope steering, resistive triggers on DualSense, `SENSITIVITY` 0.5,
  `TRIGGERS_FEEDBACK` 1), **Mobile** (phone gyroscope through `AcTools.GamepadServer.exe`, a WebSocket server, and the
  memory-mapped file `AcTools.GamepadState.v1`; QR codes; fetches `https://acstuff.club/j/config.json`). `ac.loadRenderThreadModule` and
  `ac.loadGamepadThreadModule` (works while paused) give the script two more threads.

### 1.6 Keyboard and mouse `[dll][ini]`

* `KeyboardCarControl::KeyboardCarControl` (**C**, 0x6063c0 -> 0x6063f0, 900 B); `acquireControls` (**B**, 0x607490):
  original wrapped by 0x606c40 (53 B thunk) with the CSP record (RVA 0x2690ce8, index = byte at provider+0x73):
  the steer is blended over 0.1 s when mouse steering is toggled; a second gas source `__EXT_KEYBOARD_GAS_RAW`
  (`KEY`, `KEY_MODIFICATOR`, `MOUSE`, `MOUSE_MODIFICATOR`, `LAG_UP` 0.5, `LAG_DOWN` 0.2, `OVERRIDE` 0) is smoothed
  with its own lag and merged by max (or replaces the normal gas with `OVERRIDE`); the keyboard clutch
  `__EXT_KEYBOARD_CLUTCH` (helper 0xeceea0, 453 B) gives pads and keyboards a clutch (owner binds `XBOXBUTTON=A`);
  `[__EXT_KEYBOARD] SHIFT_WITH_XBUTTONS`, `SHIFT_WITH_WHEEL` (mouse extra buttons / wheel shift gears).
  `getAction` (**C**, 0x6067d0) reads the key codes at provider +0xa8..+0xcc through CSP's `isKeyDown` (0x6c8ac0)
  and runs the handler list 0x2690cb8. `getPointAtDistance` (**B**, 0x607680): when the AI-line helper
  (RVA 0x26a36f8) says the distance is within half its length it calls the original, otherwise it returns the
  car's own position + offset (the keyboard look-ahead on the AI spline degrades gracefully, e.g. cars without AI
  data).
* **Mouse steering script** (`general.ini [CONTROL_MOUSE_STEERING] USE_SCRIPT=0`, `IMPLEMENTATION=base`):
  `extension/lua/mouse-steering/<name>/assist.lua`, `script.update(dt, deltaX, deltaY, useMouseButtons)` with
  `ac.getJoypadState()`; shipped `base` (custom implementation: wheel = gears, X2 = handbrake), `omsi` (move up =
  accelerate, move down = brake, `SCALE_Y 5`), `wooting` (analog keyboard: `AcTools.Wooting.exe` plug-in, memory-mapped
  file `AcTools.CSP.Wooting.v0`, `[GAMMA]` keys per axis; manifest `MOUSELESS = 1` lets it run without mouse
  steering). Other keys: `RESTORE_MOUSE_POSITION=1`, `USE_RAW_INPUT=0` (`csp_raw_input` window, `RegisterRawInputDevices`),
  `TOGGLE_WITH_MIDDLE_BUTTON=0`.
* **MouseHider**: `MouseHider::MouseHider` is B (original + reads `HIDE_INTERVAL`-like value, and drops two
  vector entries), `update` and `showCursor` are replaced by the one-byte `ret` stub 0x2db1f0 (AC's own cursor
  hiding is off; CSP's USER32 `ShowCursor`, `SetCursor` (stub 0x305980), `SetCursorPos` 0xf4b220, `GetCursorPos`
  0xb70500 hooks and its ImGui layer take over), `forceMouseHide` stores the flag. File `system\cfg\mouse_hider.ini`
  is read by 0x14de820 / 0x14e2e30 / 0x14f1a60.
* **Steering extrapolation** (`[STEERING_EXTRAPOLATION]`, shared by the three providers): pre-step 0xecf070 (104 B),
  post-step 0xecf1a0 (2,113 B; refers to `general`): keys `PHYSICS_LEAD_TIME` 0 ms, `GRAPHICS_LEAD_FRAMES` 0,
  `RATE_SMOOTH_HZ` 250, `REVERSE_FADE_TIME` 10 ms, `MAX_RATE` 12; all off by default.
* **Holding handbrake** (`[HOLDING_HANDBRAKE] ENABLED=0`, `DELAY 3 s`, `SPEED_THRESHOLD 0.5`, `AUTO_DISENGAGE 0`):
  keeps the handbrake engaged after stopping; implementer not found (0x138ded0 only reads the keys).
* **Key dispatch.** `Sim::onKeyDown` (0x11beae0, 1,811 B, **C**) runs its own handler list (RVA 0x26978d8), drops
  keys while a CSP text field / UI captures the keyboard, applies `UNBIND_TURBO_DIGITS` (digits do not change turbo)
  and `UNBIND_GAMEPAD_SHORTCUT` (Ctrl+P), implements "Stop your car to start instant replay", then the original.
  `ksgui::GUI::onKeyDown` / `onMouse*` hooks belong to the UI brief. The USER32 `GetAsyncKeyState` hook (0x6c86d0,
  243 B) answers modifiers and the `Y` / `N` poll votes from CSP's table (`__CM_ONLINE_POLL_YES / _NO`) `[interp]`.

### 1.7 DualShock, DualSense, Joy-Con and Steam Input `[dll][ini][log-csp]`

* Switches: `gamepad_fx.ini [ADVANCED_GAMEPADS]` `ENABLED` 1, `BLOCK_STEAM` 1, `AUTODETECT` 1 (DualSense),
  `AUTODETECT_DUALSHOCK` 1, `REMAP_JOYCONS` 1, `SHIFTING_COLORS=SMOOTH`, `SESSION_START` 1 (start lights on the
  light bar), `EXTRA_COLORS` 1, `BACKGROUND_COLORS` 1, `MOUSE_PAD` 1, `LOW_BATTERY_WARNING` 1.
* Code: C++ classes in namespace `AC`: `Joypad`, `Joypad_remapped`, `BlankJoypad`, `DualShockJoypad`,
  `DualSenseJoypad`, `X360SecondaryJoypad` (RTTI names). DualSense HID enumeration by SETUPAPI + HID.DLL
  (0x13eb460, 3,205 B: "Trying to enumerate devices looking for DualSense gamepads", VID / PID, USB vs BT report
  length), init through `HidD_GetFeature` (0x6bc840, 728 B, "Failed to initialize DualSense controller: "),
  state exchange "DualSense: setting / getting device state". DualShock and Switch-style pads: a HID driver with the
  strings of a JoyShock-style library ("Doing handshake...", "Switching baudrate...", "Enabling IMU data...",
  "Getting calibration data...", "Controller %d timed out", 0x15ec400, 0x15ee97e) [interp] and a
  `hidapi` copy (`hid_init`, `HidD_*`, 0x16bb010). Gamepad index space for Lua: 0..3 Xbox, 4..7 DualSense, 8..11 DualShock.
  XInput 1.4 is loaded separately for the battery state (`XInputGetBatteryInformation`).
* **Haptics** (`[ADVANCED_GAMEPADS_HAPTICS] MODE 1, GAIN 1, AVAILABLE_IN_WHEEL_MODE 1`; hidden mixes `_1` minimal and
  `_2` full with 12 sources each: `BODYWORK ENGINE GEAR GEAR_GRIND LIMITER SKID WHEEL COLLISION_CAR /_OBJECT /_TRACK
  SCRAPE_CAR /_TRACK` plus `ENGINE_NORMALIZATION_OFFSET_THROTTLE / _RPM`): audio is routed to the DualSense's
  4-channel USB audio endpoint ("Building haptic feedback for gamepad #", "[DSHF] Too few audio devices to expect
  to find DualSense in there", "CSP: DualSense haptic feedback"; 0x1db13a0, 0x1db8940). Needs a USB cable.
* **Lua output API**: `ac.setDualSense / setDualShock / setXbox(index, priority, holdFor)` return mutable output
  structs (light bar, LEDs), `ac.setDualSenseTrigger{NoEffect, ContinuousResitanceEffect, SectionResitanceEffect,
  ExtendedEffect}`, `ac.setDualSenseHapticParams`; helper libraries `shared/dualsense/accent` and `grab` (stops the
  pad from moving the mouse). Lua plug-in `dualsense_lights` (5,583 B bytecode) drives the light bar from sim state.
* **Steam Input**: `\steam_api64\SteamController` is a lookup (RVA 0x268b888); `BLOCK_STEAM` tries to stop Steam
  Input from turning complex pads into plain Xbox pads (needs Steam Input off for AC to be fully effective).
* Owner's log: "Looking for DualShock-compatible devices: 166639.10 us", repeated "Looking for new DualSense
  controllers: 0", "DualShock rescan: 0" every ~1 s; no DualSense present.

### 1.8 Extra bindings (`__EXT_*`, `__CM_*`) `[ini][dll]`

* AC's own 75 sections plus CSP's; the owner's `controls.ini` has 333 sections: 75 AC, 22 `__CM_*` (Content Manager
  system commands), 236 `__EXT_*`. `data_manifest.ini` declares 12 hidden `[INPUT_GROUP_n]` tables with 116 named
  bindings: Miscellaneous 15, Car 20, Car (extended physics) 7, Car (wipers) 7, Car (extras A-T) 20, Neck FX 3,
  Freer Camera 13, Audio buttons 2, Onboard camera (Small Tweaks) 15, Real Mirrors 6, Mouse control (focus
  movement: D-pad jumps between UI items, other buttons click) 5, Music (media keys) 3.
* Examples (name = meaning): `__EXT_SIM_PAUSE`, `__EXT_TOGGLE_FULLSCREEN` (Alt+Enter), `__EXT_VIRTUAL_MIRROR` (F11),
  `__EXT_PUSHTOTALK`, `__EXT_SAVE_CLIP` (Ctrl+Shift+S), `__EXT_HIDE_UI`, `__EXT_TOGGLE_TASKBAR`,
  `__EXT_TURNSIGNAL_LEFT/RIGHT/CANCEL`, `__EXT_HAZARDS`, `__EXT_LOW_BEAM`, `__EXT_HEADLIGHTS_SWITCH`, `__EXT_DRL`,
  `__EXT_GEAR_NEUTRAL/UP/DOWN`, `__EXT_SWITCH_HSHIFTER`, `__EXT_KEYBOARD_CLUTCH`, `__EXT_PIT_LIMITER`,
  `__EXT_STARTER` (engine start for the STALLING experiment), `__EXT_FFB_*`, `__EXT_CMD_RESET`,
  `__EXT_CMD_STEP_BACK`, `__EXT_ENGINEMAP_UP`, `__EXT_TC2_UP/DOWN`, `__EXT_DRIVER_SHIFT_*`, `__EXT_SPEED_LIMITER`,
  `__EXT_MANUAL_CUTOFF`, `__EXT_WIPERS_*` (7), `__EXT_LIGHT_A..T` (20), `__EXT_LOOK_LEFT/RIGHT/BACK`,
  `__EXT_FREECAM_*` (13), `__EXT_SEAT_*` (15), `__EXT_REALMIRRORS_*` (6), `__EXT_FOCUSMOVEMENT_*` (5),
  `__EXT_MUSIC_*` (3), `__EXT_AUDIO_BTN_0/1`, `__CM_ONLINE_POLL_YES/NO`. In the owner's file the car scripts
  have added `__EXT_CAR_*` families (brake balance maps 12, brake bias coarse 12, deployment strategy 12, diff
  entry / mid / exit 36, display pages 6, pedal map 5, PU mode 14, brake migration 5, brake shape maps 5, kers
  overtake, relearn ...). Strings of 92 distinct `__EXT_` / `__CM_` names are in the DLL image (the rest are
  created at run time by `ac.ControlButton(id, defaults)`).
* Moving parts: `ac.ControlButton(id, defaults)` (Lua; 12 methods) creates a section on demand; `REPEAT_PERIOD` and
  the "hold" switch live in `controls.ini`; the in-game editor (0x134e3e0, 10,951 B, strings "Unbind gamepad",
  "keyboard & gamepad") edits keyboard, gamepad and DirectInput bindings.
  The owner's installed Lua app `apps/lua/ext_controls` ("Extended Controls", VRC, 3.0.0, 17 files) lists them in
  two tables: `cfg/car_controls.ini` (34 sections in 9 tabs: Gearbox, Brakes, Assists, Lights, Wipers, Engine,
  Hybrid, Misc., Extras) and `cfg/cm_controls.ini` (64 sections in 10 tabs: Race, Replay, Audio, UI, View,
  Mirrors, Misc., Freer Cam, Onboard Cam, Online); it uses `ac.ControlButton`, `ac.store("EXT_CONTROLS_AVAILABLE")`,
  `ac.load("__EXT_CAR_MISSING_BINDS")`.
* Lua plug-ins in the DLL's mirror archive (bytecode, not read): `extra_buttons` (8,516 B), `extra_physics_buttons`
  (755 B), `stalling` (2,414 B), `reconnect_devices` (330 B), `dualsense_lights` (5,583 B).

### 1.9 TrackIR `[dll][ini][log-csp]`

CSP re-implements the TrackIR client: `TrackIR::TrackIR` (0x112b780, **C**, scale 91.02 = 32768 / 360) queues an
init lambda only when neither the global "no head tracking" byte nor the VR byte is set; init (0x112b810,
1,798 B) reads the NaturalPoint path from `HKCU\Software\NaturalPoint\NATURALPOINT\NPClient Location`, loads
`NPClient64.dll`, **verifies its Authenticode signature** ("signature access failed" / "signature verification
failed"; uses `NP_GetSignature`), calls `NP_RegisterWindowHandle`, `NP_RegisterProgramProfileID`, `NP_QueryVersion`
("NaturalPoint software version is %d.%02d"), `NP_StopCursor`, `NP_RequestData`, `NP_StartDataTransmission`,
`NP_GetData`, `NP_ReCenter`, `NP_StartCursor`, `NP_UnregisterWindowHandle`, `NP_StopDataTransmission` (0x112b020 registry,
0x112b260 loader 1,140 B, 0x112bf20 shutdown). `TrackIR::getOffsets` (0x112bf90, 1,766 B, **B**): original when
`[small_tweaks] [TRACK_IR] ALT_IMPLEMENTATION` is 0; otherwise reads `NP_GetData`, treats data older than
500 ms as lost ("[TrackIR] Failed to get data" / "Reestablished connection"), fades in and out, converts with the
raw-to-normalised factor 1e-5 * 9.09 * 0.11, applies `POSITION_SCALE`, `POSITION_OFFSET`, `ROTATION_SCALE`,
`ROTATION_OFFSET` and runs event bus 0x26974f0. `TrackIR::~TrackIR` is the `ret` stub. The Lua value
`ac.getTrackIR()` (`ac.StateTrackir`) exposes it; `neck.ini [BASIC] ENABLED_WITH_TRACKIR` and
`[LOOK] LOOK_WITH_RHM` (Real Head Motion) decide how Neck FX combines with it. `cfg/trackir.ini [TRACK_IR] VALUE`
(owner: 100, "max rotation = 36000 / VALUE") is AC's own. Owner's log: "[TrackIR] Location is not set in registry"
(no TrackIR installed).

### 1.10 Hardware extras

* **Logitech wheels**: CSP loads the Logitech Steering Wheel SDK through the CLSID
  `{e8dfb59f-141f-40e4-8dd4-5526ead25a4c}`, checks the signer ("Logitech Inc", CryptoAPI
  `CertGetNameStringW`) and binds **51 function names** (`logiWheel*` 20 incl. `SetOperatingRange*`, `PlayLeds`,
  `SetRpmLeds`; `logiTrueForce*` 30 incl. `SetTorqueKF`, `SetTorqueTF*`, `SetStreamTF`, `GetHapticRate`;
  `logiAdvancedGetThreadHandles`; 0x1cd3cb0, 1,716 B). Log strings: "Logitech wheel found / connected / with TF /
  max torque", "Set Logitech hardware lock: " (0x15cb930, 1,123 B), "Failed to set Logitech hardware lock". The
  hardware lock sets the wheel's operating range to the car's steering lock `[interp]`. What CSP sends through
  TrueForce is not mapped.
* **G27 / G29 LEDs**: `g27_lights.ini` (module function 0x1224300, 2,188 B): `MODE=DI_BASED` (mirror the car's
  digital-instrument "virtual LEDs" `RPM_START`, `RPM_SWITCH`, `BLINK_SWITCH`, `GEARS`; falls back to
  `DI_BASED_FALLBACK_TO_DISABLED`), `PERCENTAGE` (`[PERCENTAGE] MIN 0.5, MAX 0.95`), `AI_BASED` (AI shifting
  range) or `DISABLED`; `CUSTOM_DELAY=12` frames; `ALLOW_PYTHON_OVERRIDE=0` with Python
  `ac.ext_setG27Thresholds`, `ac.ext_isG27Available`. Sits on handler list 0x2690c78 of `InputDevice::setG27Leds`.
* **Fanatec**: no CSP hook; CSP only references `system/cfg/fanatec.ini` (0xed06a0, 966 B, which also names
  `controls.ini` and `system/cfg/assetto_corsa.ini`: the set of files whose change reloads the control settings,
  Lua `ac.onControlSettingsChanged`, `ac.reloadControlSettings`) [interp]. **Moza, Simucube, Thrustmaster,
  Simagic**: no string found. **OpenRGB**: `internal/plugins/AcTools.OpenRGB.exe` (213,728 B) ships but no
  reference in the DLL image or in plain Lua [interp: used from compiled Lua]. **Wooting**: see 1.6. **VR hands**:
  `ac.setVRHandVibration`, `ovr_SetControllerVibrationPtr`, OpenXR `xrApplyHapticFeedback` (VR brief).

## 2. Game parts it replaces or extends

Source: `hook_groups.tsv` rows of group `audio_input` for `ksDI.lib` and the input classes, plus the FFB rows of
`physics_car`, `misc_core` and `rendering_hooks`. **43 hooks and 8 lookups** in the 14 classes below (`attach.md`
2.2 lists 31 / 16 because it files DICarControl 4/2, X360Joypad and MouseHider differently; the numbers here are
counted row by row). "Hook fn" = replacement RVA, size from the exception table (`dll_funcs.tsv`), "slot" = data
slot of the original.

### 2.1 Device layer, `ksDI.lib` (11 hooks + 2 lookups)

| Symbol | acs VA | Hook fn (size) / slot | Cat | What it changes |
|---|---|---|---|---|
| `DirectInput::DirectInput` | 0x14023e8b0 | 0x5ae370 (422) / 0x268abf8 | B | guard + log + watchdog thread, post-setup 0x5b4170 |
| `DirectInput::poll` | 0x14023f160 | 0x5ae5c0 (20 -> body) / 0x268abe0 | C | own device loop, async snapshot copy, paused flag 0x26a132f |
| `DirectInput::initJoys` #1 | 0x14023f030 | 0x5b3c10 (265) / 0x26a48b0 | B | original, "Devices are enumerated: N", order fix 0x5b1df0, listener bus 0x2690b68. The active variant (log lines) |
| `DirectInput::initJoys` #2 | same | 0x5b4010 (23) / 0x26a48c0 | B | same without logging; second registration of one function, only one of the two can be live (cf. `Car::pollControls` double registration in `physics_car_hooks.md`) |
| `DirectInput::~DirectInput` | 0x14023e9a0 | 0x5b3d20 (266) / 0x26a48b8 | B | shutdown byte 0x268a7af, own cleanup skipping placeholders |
| `DirectInput::cleanup` | 0x14023ef60 | lookup / 0x268abe8 | D | called by CSP |
| `InputDevice::InputDevice` | 0x14023f2f0 | 0x5fb330 (501) -> 0x5fafc0 / 0x26a4c18 | C | own constructor, placeholder on failure, CSP map 0x25e1628 |
| `InputDevice::initFF` | 0x14023f580 | 0x5fab00 (1,208) / 0x26a4c10 | B / C | handler bus 0x2690c80; CUSTOM_FFB_INIT |
| `InputDevice::poll` | 0x14023f780 | 0x5fc060 (thunk) -> 0x5fc070 / 0x26a4ce8 | C | reconnect counter, backoff, snapshot path |
| `InputDevice::sendFF` | 0x14023f9b0 | 0x5fc2b0 (105) / 0x26a4d00 | C | bus 0x2690c88, 0x5fce20, async / adaptive |
| `InputDevice::setG27Leds` | 0x14023fb80 | 0x5fc1a0 (51) / 0x26a4cf0 | B | bus 0x2690c78 may cancel / rewrite |
| `InputDevice::~InputDevice` | 0x14023f4a0 | 0x5fcf10 (112) / 0x26a4d08 + lookup 0x268abd8 | C | removes the device from CSP's map; original destructor body only via the lookup |

### 2.2 Wheel provider (`DICarControl.obj`, `DICommandManager.obj`, `DIControl*.obj`: 11 hooks + 1 lookup)

| Symbol | acs VA | Hook fn (size) / slot | Cat | What it changes |
|---|---|---|---|---|
| `DICarControl::DICarControl` | 0x14007ddd0 | 0x1096dc0 (329) / 0x268bb58 | B | per-provider record, registration |
| `DICarControl::~DICarControl` | 0x14007fbe0 | 0x1096f10 (27) / 0x26aaf30 | A | clears the singleton pointer, tail-calls original |
| `DICarControl::acquireControls` | 0x14007fe70 | 0x1097490 (677) / 0x268bb48 | B | async snapshot copy for the steering device, steering extrapolation, bus 0x2695f50; while the CSP extra gear buttons are in use, bit 0x80 ("disabled") of the two gear cells at provider +0x250 / +0x2a0 is set around the original call so one press does not shift twice [interp] |
| `DICarControl::getAction` | 0x1400805d0 | 0x1097740 (314) / 0x268bb50 | B | look axis, horn toggle, global no-input byte |
| `DICarControl::sendFF` | 0x140080e40 | 0x1097880 (119) / 0x26aaf38 | B | offset, averaging of skipped steps |
| `DICarControl::setVibrations` | 0x140081010 | 0x1097900 (181) / 0x26aaf40 | C | CLEAN_FFB mix |
| `DICommandManager::step` | 0x140081730 | 0x5a7260 (60) / 0x268ab68 | C | gated key posting |
| `DIControlAxis::DIControlAxis` | 0x140081800 | lookup / 0x268ab70 | D | called when CSP builds extra axes |
| `DIControlButton::DIControlButton` | 0x140081b40 | 0x5a7450 (49) / 0x268ab98 | B | sets flag bits 7 (no POV, no modifier) |
| `DIControlButton::load` | 0x140081cc0 | 0x5a7490 (2,916) / 0x268ab80 | B | `ENABLED`, `KEY_MODIFICATOR`, `__CM_POV`, `POV_DIR`, `__ALT_` |
| `DIControlButton::getValue` | 0x140081c90 | 0x5a8000 (291) / 0x268ab88 | C | POV / modifier / alternatives |
| `DIControlButton::getKeyboardValue` | 0x140081c50 | 0x5a8130 (35) / 0x268ab78 | C | alternatives, key state table |

### 2.3 Gamepad, keyboard, mouse, TrackIR, FFB app (21 hooks + 4 lookups)

| Symbol | acs VA | Hook fn / slot | Cat | What it changes |
|---|---|---|---|---|
| `X360Joypad::acquire` | 0x140243ff0 | 0x6bf270 (106) / 0x26a85f0 | C | injected mask, D-pad blanking |
| `X360Joypad::setVibrations` | 0x140244110 | 0x6bf260 / 0x26a85e8 | C | clamp, TLS mark for the IOCTL hook |
| `X360Joypad::getButtonMask` | 0x140244010 | lookup / 0x268b858 | D | |
| `JoypadCarControl::JoypadCarControl` | 0x1401d9280 | 0x601b50 (40) -> 0x601b80 / 0x268b218 | C | own ctor |
| `JoypadCarControl::~JoypadCarControl` | 0x1401dbae0 | 0x602430 (326) / 0x26a4d80 | B | usage counters per gamepad type |
| `JoypadCarControl::acquireControls` | 0x1401dbce0 | 0x602b60 (426) / 0x268b208 | B | record, script bus 0x2690c98 |
| `JoypadCarControl::getAction` | 0x1401dc130 | 0x602d10 (40) / 0x268b1f8 | B | bus 0x2690ca0, horn toggle |
| `JoypadCarControl::getAxisValue` | 0x1401dc250 | lookup / 0x268b200 | D | |
| `JoypadCarControl::sendFF` | 0x1401dc370 | 0x602eb0 (96) / 0x26a4d88 | C | 10 / 5 step rate |
| `JoypadCarControl::setVibrations` | 0x1401dc3c0 | 0x602f10 / 0x26a4d90 | C | skip bit, FLT_MIN floor |
| `SecondaryJoypadButton::isPressed` | 0x1401dc300 | 0x69d7d0 / 0x268b518 | C | key table, UI capture |
| `KeyboardCarControl::KeyboardCarControl` | 0x14010f0e0 | 0x6063c0 (40) -> 0x6063f0 / 0x268b258 | C | own ctor |
| `KeyboardCarControl::acquireControls` | 0x14010f2e0 | 0x607490 (86) / 0x268b228 | B | gas raw, clutch, steer blend |
| `KeyboardCarControl::getAction` | 0x14010f820 | 0x6067d0 (64) / 0x268b230 | C | bus 0x2690cb8, horn toggle |
| `KeyboardCarControl::getPointAtDistance` | 0x14010fc50 | 0x607680 / 0x26a4da0 | B | fallback position |
| `KeyboardCarControl::getKeyboardSteering`, `computeGasCoefficient` | 0x14010faa0, 0x14010f6f0 | lookups / 0x268b240, 0x268b248 | D | |
| `MouseHider::MouseHider` | 0x140114850 | 0x6236a0 (184) / 0x26a4fa8 | B | |
| `MouseHider::update`, `showCursor` | 0x140114ed0, 0x140114e20 | 0x2db1f0 (1) / 0x26a4fb0, 0x26a4fc8 | C | **no-ops** (`ret`) |
| `MouseHider::forceMouseHide` | 0x1401992d0 | 0x6238b0 / 0x26a4fc0 | C | stores flag (`sim.obj`) |
| `TrackIR::TrackIR` | 0x1401cdfe0 | 0x112b780 (45) / 0x26ad170 | C | own client |
| `TrackIR::getOffsets` | 0x1401ce530 | 0x112bf90 (1,766) / 0x268c4f8 | B | filtered offsets, bus 0x26974f0 |
| `TrackIR::~TrackIR` | 0x1401ce2a0 | 0x2db1f0 / 0x26ad178 | C | **no-op** |
| `FFBForm::FFBForm` | 0x140314a10 | 0x305980 / 0x26a8ff8 | C | stub returns 0 (stock FFB app is not created) |

### 2.4 FFB / controls rows of other areas (cross-reference, details in `physics_car_hooks.md`)

| Symbol | acs VA | Hook fn / slot | Cat | Relevance |
|---|---|---|---|---|
| `Car::getSteerFF` | 0x140272180 | 0x595800 (78) / 0x268aad8 | B | final FFB value; bus 0x268ff60; RANGE_COMPRESSION_ASSIST byte 0x269958a |
| `Car::pollControls` #1 / #2 | 0x140274e70 | 0x59fdb0 / 0x59fe60 (391) | B | car physics script sets controls and FF value |
| `Car::lockControlsUntil` | 0x140274600 | 0x59f4a0 (112) / 0x26a47e8 | B | control lock (also `lockControlsUntilTime` below) |
| `PhysicsEngine::initLowSpeedFF` | 0x140263c90 | lookup / 0x268b3e8 | D | AC's low-speed FFB fade; `ac.disableLowSpeedFFBReduction` |
| `SteeringSystem::step` | 0x1402b81b0 | lookup / 0x268a808 | D | |
| `CarAvatar::modifyUserFFGain` | 0x1400d90f0 | lookup | D | FFB hotkeys |
| `CarAvatar::setControlsProvider` | 0x1400d9f90 | lookup | D | CSP swaps providers (`ac.overrideCarControls`) |
| `CarAvatar::lockControlsUntilTime` | 0x1400d8e20 | 0x10aabe0 (59) / 0x268bd60 | A | bus 0x2696878 then original |
| `CarAvatar::setAutoClutchEnabled` | 0x1400d9ce0 | 0x10ac580 / 0x268bcf8 | A | remembers the user's auto-clutch choice (RVA 0x25e0730 / 0x25e0734) |
| `CarAvatar::setControlsLock` | 0x1400d9f00 | 0x10ad370 / 0x268bd10 | B | forces lock while flag byte +0xfc7 is set |
| `Sim::onKeyDown` | 0x14019a940 | 0x11beae0 (1,811) / 0x26add90 | C | see 1.6 |
| `CameraMouseControlBase::update` / ctor | 0x14020ff30 / 0x14020fc20 | 0x593f70, 0x593f30 | A / B | camera brief (bus 0x268ff48) |
| `CinematicFreeCamera::getMouseLook` | 0x14006ca60 | 0x5a6700 (477) | B | camera brief; uses CSP key table |
| `AIDriver::acquireControls` | 0x14029baa0 | 0x107f280 | B | AI brief |

### 2.5 Module-qualified names (not in `csp_hook_table.tsv`; resolved from the registration functions)

| Name | Registration | Replacement / slot | Kind |
|---|---|---|---|
| `\XINPUT1_3\XInputGetState` | 0x260310 (211 B) | slot RVA 0x268b848 | **lookup** |
| `\XINPUT1_3\XInputSetState` | 0x2603f0 (211 B) | slot RVA 0x268b850 | **lookup** |
| `\KERNEL32\DeviceIoControl` | 0x2817f0 (232 B) | 0xf81310 (737 B) / 0x26aa790 | hook |
| `\USER32\GetAsyncKeyState` | 0x261100 | 0x6c86d0 (243 B) / 0x26a8658 | hook |
| `\USER32\GetCursorPos`, `SetCursorPos`, `ShowCursor`, `SetCursor` | 0x26c0d0, 0x2813b0, 0x281570, 0x2811e0 | 0xb70500 (73 B), 0xf4b220 (51 B), 0xf4b290, 0x305980 (stub) | hook |
| `\steam_api64\SteamController` | 0x269800 | slot RVA 0x268b888 | lookup |

The bodies of the module-qualified hooks were not decompiled (decompile requests).

### 2.6 CSP event buses on the input path

13 lists of 0x28-byte entries (callback at +8, called last-registered first) at `.data` RVAs 0x2690b68
(`initJoys` listeners), 0x2690c78 (G27 LEDs), 0x2690c80 (`initFF`), 0x2690c88 (`sendFF`), 0x2690c98 (pad
`acquireControls`), 0x2690ca0 (pad `getAction`), 0x2690cb8 (keyboard `getAction`), 0x2695f50 (wheel
`acquireControls`), 0x268ff60 (`getSteerFF`), 0x268ff48 (camera mouse), 0x26974f0 (TrackIR offsets), 0x26978d8
(`Sim::onKeyDown`), 0x2696878 (`lockControlsUntilTime`). The Lua scripts, the ffb_tweaks / gamepad_fx / g27_lights
modules and the CSP UI register their handlers there [interp: the registrars were not read].

## 3. Config keys and Lua API

### 3.1 `extension/config/ffb_tweaks.ini` (24 functional keys, 5,033 B; defaults)

| Section | Key = default | Shipped comment (own words) |
|---|---|---|
| `[BASIC]` | `ENABLED=1` | allow CSP to alter FFB |
| `[CUSTOM_SOFT_LOCK]` | `ENABLED=1`, `PADDING=10` (4..20 deg), `SHIFT_PADDING=0.8`, `FORCE_FACTOR=1` (0..2), `SPEED_FACTOR=1` (0..2), `DAMPER=0` | smoother, less bouncy lock; padding = zone in which force ramps; shift = lock may go outside for cars below the wheel's lock |
| `[POSTPROCESSING]` | `RANGE_COMPRESSION=1.0` (0.5..4), `RANGE_COMPRESSION_ASSIST=0`, `CLEAN_FFB=1`, `DIRT_VIBRATION=0.0` (0..1.5); hidden `GAMMA`, `DIRT_VIBRATION_FREQUENCY` 0.2, `_VARIATION` 1,3, `_REMAP` 0,1 | compress force so small forces are felt; convert STEER_ASSIST; remove doubled effects on custom-physics cars |
| `[POSTPROCESSING_SCRIPT]` | `ENABLED=0`, `IMPLEMENTATION=alternative` | FFB Lua script; `[POSTPROCESSING_SCRIPT_SETTINGS] __PLUGIN_SETTINGS` -> `extension/state/lua/soft_lock/{0}__settings.ini` |
| `[REAL_FEEL]` | `ENABLED=0`, `WHEEL_MAX_NM=35.0` (2..35), `ENABLE_FOR_ALL=0` | real steering forces, Direct Drive only |
| `[REAL_FEEL_BRAKE]` | `ENABLED=0`, `BRAKE_MAX_KG=0.0`, `BRAKE_MAX_INACTIVE_PERC=100.0`, `CLAMP_FORCES=1` | real pedal force, load-cell pedals only |
| `[GYRO2]` | `ENABLED=0`; commented `ORIGINAL_GYRO`, `OLD_VERSION`, `STRENGTH 0.25` | physically more accurate gyro, not for extended physics |
| `[TWEAKS]` | `ADAPTIVE_FFB=0` (0 off, 1 adjust rate, 2 async, 3 both), `AVERAGE_SKIPPED_FFB=1`, `SKIP_DAMPING_FRAMES=3` (0..5); commented `STRUT_FFB_ADJUSTMENT2=1`, `FADE_IN_AFTER_LOADING2=1` | keep FFB from slowing physics |

Plus the car-side keys the physics brief maps (`surfaces_fx.ini [FFB] SAVER, SAVER_SENSITIVITY, GRAVEL_FFB_STRENGTH,
FFB_BASE_MULT 1.3, FFB_LOW_GAIN 8, FFB_HIGH_REDUCE 8`, hidden `[DEV] EXTRA_FFB_MULT 0.85`; car data
`FFB_LOW_PASS_HZ`, `ADD_TO_FFB`, `FIX_STRUT_FFB`).

### 3.2 `extension/config/gamepad_fx.ini` (56 lines incl. header; 47 functional keys)

`[BASIC] ENABLED=1`, `BOOST_FREQUENCY=0`, hidden `DEBUG=0`; `[AXIS_REMAP]` (empty, hint only); `[JOYPAD_ASSIST]
ENABLED=0`, `IMPLEMENTATION=gamepad_force`; `[JOYPAD_ASSIST_SETTINGS] __PLUGIN_SETTINGS` -> `extension/state/lua/
joypad_assist/{0}__settings.ini`; `[ADVANCED_GAMEPADS]` 11 keys (section 1.7); `[ADVANCED_GAMEPADS_HAPTICS] MODE=1,
GAIN=1, AVAILABLE_IN_WHEEL_MODE=1`; hidden `[..._HAPTICS_1]` and `[..._HAPTICS_2]` with 14 keys each.

### 3.3 `g27_lights.ini` (6 functional keys)

`[BASIC] ENABLED=1`, `MODE=DI_BASED`, `CUSTOM_DELAY=12`, `ALLOW_PYTHON_OVERRIDE=0`; `[PERCENTAGE] MIN=0.5`, `MAX=0.95`.

### 3.4 `general.ini` input sections (defaults)

| Section | Keys |
|---|---|
| `[CONTROL]` | `LOOK_OUT_FOR_MISSING=0`, `RELEASE_DPAD=0`, `UNBIND_TURBO_DIGITS=0`, `UNBIND_GAMEPAD_SHORTCUT=1`, `ALTERNATIVE_AI_DRIVER=1`, `AI_DRIVER_KEEP_SETTINGS=1`, `IGNORE_ESCAPE_IN_REPLAYS=0`, `INACTIVE_ONLY_WHEN_COVERED2=2`; commented `FIX_ORDER2=1`, `QPM_FIX_WINGS2=1`, `DISCORD_HOOK_FIX2=1`, `BINARY_MODIFIERS_FILTERING2=1` |
| `[CONTROL_MOUSE_STEERING]` | `RESTORE_MOUSE_POSITION=1`, `USE_RAW_INPUT=0`, `TOGGLE_WITH_MIDDLE_BUTTON=0`, `USE_SCRIPT=0`, `IMPLEMENTATION=base` (+ `_SETTINGS`) |
| `[STEERING_EXTRAPOLATION]` | `PHYSICS_LEAD_TIME=0`, `GRAPHICS_LEAD_FRAMES=0`, `RATE_SMOOTH_HZ=250`, `REVERSE_FADE_TIME=10`, `MAX_RATE=12` |
| `[HOLDING_HANDBRAKE]` | `ENABLED=0`, `DELAY=3`, `SPEED_THRESHOLD=0.5`, `AUTO_DISENGAGE=0` |
| `[INPUTS_REMAP]` (hidden) | `OVERRIDE_CAR_FFMULT=0`, `OVERRIDE_CAR_STEERASSIST=0`, `SKIP_THROTTLE_CURVES=0` |
| `[OTHER_EXPERIMENTS]` | `CUSTOM_FFB_INIT=0`, `IMPROVED_MODIFIERS_HANDLING=1` (modifier keys change state only when the main button is not held) |
| `[DEV]` | `WRAP_HOOKS=1` (stops system-wide keyboard / mouse hooks of other tools while AC is in the background) |

### 3.5 Other files

* `small_tweaks.ini`: `[TRACK_IR] ALT_IMPLEMENTATION=1` (0 off, 1 optimisations + fixes, 2 + corrected scale),
  `POSITION_SCALE=1,1,1`, `POSITION_OFFSET=0,0,0`, `ROTATION_SCALE=1,1,1`, `ROTATION_OFFSET=0,0,0`;
  `[BASIC] RECONNECT_WHEEL_ON_APP_RESTORE=0`, `DEBUG_SYSTEM_KEYBINDINGS=0`, hidden `DISABLE_CM_HOTKEYS=0`;
  `[AUDIO_VOLUME_BUTTONS] MODE=SET_RESTORE, LEVEL_0, LEVEL_1, STEP 0.05, TRANSITION_LAG 0.9, SHOW_MESSAGE, SAVE_VOLUME`;
  `[FFB_ADJUSTMENTS] STEP=0.05, SHOW_MESSAGE=1`; `[ONBOARD_HOTKEYS]`, `[REALMIRROR_HOTKEYS]`.
* `neck.ini` (100 keys, **`[BASIC] ENABLED=0`**, `ENABLED_WITH_TRACKIR=1`, `[SCRIPT] ENABLED=1`, `[LOOK]`
  `XBOX_STICK=1` right stick looks, `XBOX_STICK_DEADZONE 0.03`, `XBOX_STICK_EXPONENT 1.8`, `MOUSE_SENSITIVITY`,
  `LOOK_WITH_RHM=0`): camera brief.
* `freer_camera.ini` (48 keys): mouse and key controls of the free camera (camera brief).
* `data_controls_props.ini`: display names of 37 `[BASIC]` and 28 `[SYSTEM]` bindings; `data_hotkeys_supported.txt`:
  the 37 Content Manager system commands CSP can bind; `data_manifest.ini [INPUT_GROUP_0..11]`.
* Documents: `cfg/controls.ini` (read and **written** by Content Manager and by the in-game editor),
  `cfg/controllers/<guid>.ini` (CM presets: `[CONTROLLER] NAME`, `PRODUCT`, per-axis overrides),
  `cfg/ff_post_process.ini`, `cfg/user_ff.ini`, `cfg/trackir.ini`, `extension/state/lua/{joypad_assist,
  mouse_steering,soft_lock}/<impl>__settings.ini` (script settings).

### 3.6 Lua API (SDK `lua_sdk_functions_union.txt`, 2,728 unique names in 19 modules)

Input-related names: **136** unique function names in the groups below (the 12 `ac.ControlButton` methods counted as
13 with the constructor).

| Group | n | Names |
|---|---|---|
| `ac.ControlButton` | 13 | ctor + `pressed`, `down`, `released`, `onPressed`, `onReleased`, `configured`, `disabled`, `setDisabled`, `setAlwaysActive`, `holdMode`, `boundTo`, `control` |
| Gamepads | 17 | `isGamepadButtonPressed`, `getGamepadAxisValue`, `getJoypadState`, `getDualSense`, `getDualShock`, `getDualSenseControllers`, `getDualShockControllers`, `setDualSense`, `setDualShock`, `setXbox`, `setDualSenseTrigger{No,ContinuousResitance,SectionResitance,Extended}Effect`, `setDualSenseHapticParams`, `setButtonPressed`, `loadGamepadThreadModule` |
| DirectInput joysticks | 14 | `getJoystickCount`, `...Name`, `...InstanceGUID`, `...ProductGUID`, `...IndexByInstanceGUID`, `...AxisCount`, `...ButtonsCount`, `...DpadsCount`, `...AxisValue`, `...DpadValue`, `...Look`, `isJoystickAxisValue`, `isJoystickButtonPressed`, `isJoystickDpadValue` |
| Controller state | 5 | `getControllerSteerValue`, `isControllerGasPressed`, `isControllerBrakePressed`, `isControllerGearUpPressed`, `isControllerGearDownPressed` |
| FFB / steering | 18 | `getFFBGain`, `getFFBTweaksReport`, `setFFBMultiplier`, `setPureFFBMultiplier`, `setSteeringFFB`, `setSteeringDamping`, `unlockFFBLimits`, `disableSoftLock`, `disableLowSpeedFFBReduction`, `getSteerAxisValue`, `getSteerInputValue`, `setSteerInputValue`, `getSteerJointError`, `getSteerJointFeedback`, `getCarPhysicsRate`, `addWheelsFeedbackTorque`, `setCustomDriverModelSteerAngle`, `physics.offsetFinalFFB` |
| Keyboard / mouse | 10 | `isKeyDown/Pressed/Released`, `sendKeyboardButtonEvent`, `setMouseLeftButtonDown`, `setMousePosition`, `setMouseWheel`, `accessMouseDelta`, `hideMouseCursor`, `blockEscapeButton` |
| Controls config / hotkeys | 9 | `INIConfig.controlsConfig`, `reloadControlSettings`, `onControlSettingsChanged`, `loadCarControlsConfiguration`, `overrideCarControls`, `overrideGasInput`, `replaceClutch`, `simulateCustomHotkeyPress`, `trySimKeyPressCommand` |
| TrackIR / VR hands | 3 | `getTrackIR`, `setVRHandVibration`, `setVRHandBusy` |
| `physics.*` input override | 15 | `forceUser{Throttle,Brakes,Clutch,Handbrake}For`, `lockUserControlsFor`, `lockUserGearboxFor`, `offsetUserSteering`, `overrideSteering`, `overrideUserSteering`, `setCarNoInput`, `setAINoInput`, `setAISteerMultiplier`, `setUserControlledCar`, `getCarInputControls`, `estimateAIControls` |
| `ui.*` keyboard / mouse | 32 | `captureKeyboard`, `captureMouse`, `hotkey{Ctrl,Alt,Shift}`, `keyPressed`, `keyboardButton{Down,Pressed,Released}`, `mouse*` (17), `ac.CapturedKeyboard` methods (8) |

Module sizes (`lua_sdk_counts.tsv`): `ac_ffb_postprocess` 517,549 B, 905 functions (110 own, 61 classes, 59 aliases;
22 functions unique to it against `ac_background`); `ac_joypad_assist` 535,842 B, 956 functions (161 own, 63 classes).
Script kinds: FFB post-process 2 shipped, gamepad / mouse assist 6 shipped (3 + 3).
Python (`extension/internal/python/functions.txt`, 213 `ac.ext_*`): input ones are `ext_isJoystickButtonPressed`,
`ext_getJoystickAxisValue`, `ext_getJoystickDpadValue`, `ext_isG27Available`, `ext_setG27Thresholds`,
`ext_getSteerTorque`, `ext_getHandbrake`.

## 4. Files it reads

* `<Documents>\Assetto Corsa\cfg\controls.ini` (all sections above; `[CONTROLLERS]` for the order fix; `[__EXTRA_CM]`
  written by Content Manager: `AUTO_ADJUST_SCALE`, `DELAY_SPECIFIC_SYSTEM_COMMANDS`, `FF_POST_PROCESS` and `SYSTEM`
  as Base64 copies of `ff_post_process.ini` and of the `[SOFT_LOCK]`, `[VR]`, `[FF_EXPERIMENTAL] ENABLE_GYRO,
  DAMPER_MIN_LEVEL, DAMPER_GAIN`, `[LOW_SPEED_FF] SPEED_KMH 3, MIN_VALUE 0.01` blocks, `HARDWARE_LOCK`,
  `SHOW_SYSTEM_DELAYS`, `SYSTEM_IGNORE_POV_IN_PITS`), `cfg\controllers\*.ini`, `cfg\ff_post_process.ini`,
  `cfg\user_ff.ini`, `cfg\trackir.ini`.
* `<AC>\system\cfg\assetto_corsa.ini` ([FORCE_FEEDBACK], [SOFT_LOCK], [FF_EXPERIMENTAL], [GAMEPAD]),
  `system\cfg\fanatec.ini`, `system\cfg\mouse_hider.ini`.
* `<AC>\extension\config\{ffb_tweaks,gamepad_fx,g27_lights,general,small_tweaks,neck,freer_camera,data_manifest,
  data_controls_props}.ini`, `data_hotkeys_supported.txt`; user overrides `<Documents>\cfg\extension\*.ini`
  (owner has none for these modules; `general.ini` there is 0 bytes).
* `<AC>\extension\lua\{joypad-assist,ffb-postprocess,mouse-steering}\<impl>\{manifest.ini,assist.lua|ffb.lua,
  settings.ini}`; `extension\internal\lua-shared\dualsense\{accent,grab}.lua`; `extension\internal\plugins\
  AcTools.{GamepadServer,Wooting,OpenRGB}.exe`.
* Registry `HKCU\Software\NaturalPoint\NATURALPOINT\NPClient Location` and the `NPClient64.dll` it names;
  `HKCR\CLSID\{e8dfb59f-...}` for the Logitech SDK DLL; `Xinput1_4.dll`; `HID.DLL`, `SETUPAPI.dll`; Windows device
  interfaces (HID) for DualSense.
* Car data: `car.ini [CONTROLS] FFMULT, STEER_ASSIST` (and `[INPUTS_REMAP]` overrides), `suspensions.ini`
  `ADD_TO_FFB`, `FFB_LOW_PASS_HZ`; none of the owner's 204 cars or 26 tracks carries an input / FFB key in
  `ext_config.ini` (`content_scan`).
* Writes: `controls.ini` sections created by `ac.ControlButton` (through Content Manager / the editor),
  `extension\state\lua\*\*__settings.ini`, the CSP log.

## 5. How it is switched on

| Feature | Switch | Default | On this PC (no overrides for these modules) |
|---|---|---|---|
| Everything | `general.ini [BASIC] ENABLED` | 1 | on |
| FFB tweaks module | `ffb_tweaks.ini [BASIC] ENABLED` | 1 | on; no force-feedback device, so the output side is idle; `Car::getSteerFF` still runs for car 0 |
| Custom soft lock, clean FFB | `[CUSTOM_SOFT_LOCK] ENABLED`, `[POSTPROCESSING] CLEAN_FFB` | 1, 1 | on, wheel only |
| FFB script, real feel, gyro2, range compression | their `ENABLED` keys | 0 | off |
| Async / adaptive FFB, custom init | `ADAPTIVE_FFB`, `CUSTOM_FFB_INIT` | 0, 0 | off |
| Averaging skipped FFB | `AVERAGE_SKIPPED_FFB` | 1 | on (needs `FF_SKIP_STEPS > 0`; owner's value 0) |
| Gamepad FX device layer | `gamepad_fx.ini [BASIC] ENABLED`, `[ADVANCED_GAMEPADS] ENABLED` | 1, 1 | on, scans for DualShock / DualSense at start-up and about once per second; none present |
| Gamepad script | `[JOYPAD_ASSIST] ENABLED` | 0 | off |
| Mouse steering script | `[CONTROL_MOUSE_STEERING] USE_SCRIPT` | 0 | off |
| Order fix, placeholders | automatic (`FIX_ORDER2` hidden, default 1) | on | on: "does not need fixing" |
| Hot-plug | `LOOK_OUT_FOR_MISSING` | 0 | off |
| Steering extrapolation, holding handbrake, input remaps | their keys | 0 | off |
| Ctrl+P unbound | `UNBIND_GAMEPAD_SHORTCUT` | 1 | on |
| Pad clutch | `controls.ini [__EXT_KEYBOARD_CLUTCH] XBOXBUTTON=A` | unbound | **on** (owner binds A); also `[__EXT_CAR_KERS_OVERTAKE] XBOXBUTTON=B` |
| Pad rumble | `controls.ini [X360] RUMBLE_INTENSITY=1`, `JOYPAD_INDEX=0` | | on |
| G27 LEDs | `g27_lights.ini [BASIC] ENABLED` | 1 | on, no G27 |
| TrackIR | `[TRACK_IR] ALT_IMPLEMENTATION` | 1 | looks for TrackIR; not installed |
| Neck FX | `neck.ini [BASIC] ENABLED` | **0** | off |
| Logitech SDK | automatic when a Logitech wheel is found | | not found |
| Input method | `controls.ini [HEADER] INPUT_METHOD` | | `X360` |
| Server restriction | acServer rule shown by 0x10ab7e0: "Chosen input method is not available on this server, only %ls allowed" | | |

## 6. Size

| Item | Number |
|---|---|
| Hooked rows in the input classes | 43 hooks + 8 lookups (14 classes); + 12 hook / 7 lookup rows in other areas that touch input (2.4); 6 module-qualified registrations (2 XInput lookups, 1 Steam lookup, `DeviceIoControl`, `GetAsyncKeyState`, 4 cursor hooks) |
| Pseudo-C read | 58 replacement functions, 148,233 bytes (`decomp_hooks/`); largest: `DIControlButton::load` 17,735 B, `59fdb0` 15,695 B, `InputDevice::poll` 10,128 B |
| CSP-own input functions identified by strings, **not** decompiled | about 40: 0x5fafc0, 0x5fb530, 0x5fce20, 0x5fc720, 0x5fcae0, 0x5ae870 (4,060 B), 0x5af860, 0x5b1df0 (6,082 B), 0x5b0370, 0x5b1bb0, 0x601b80, 0x601480, 0x601f50, 0x6063f0, 0x1097d10, 0xecf070 / 0xecf1a0, 0x13eb460, 0x6bc840, 0x15ec400, 0x15ee97e, 0x16bb010, 0x15cb930, 0x15cbda0, 0x1cd3cb0, 0xcb87a0 (5,620 B), 0xcb8280, 0x415870, 0x4154a0, 0x1224300 (2,188 B), 0x112b810, 0x112b260, 0xf81310, 0x6c86d0, 0xed06a0, 0xed0a70, 0xd0f480, 0xeceea0, 0x134e3e0 (10,951 B) |
| Registered handler buses | 13 |
| Config keys | ffb_tweaks 24 + gamepad_fx 47 + g27_lights 6 + general input sections 8 + 5 + 5 + 4 + 3 + 2 + 1 + small_tweaks 13 |
| Binding sections | 116 named in `data_manifest.ini`; 92 `__EXT_` / `__CM_` name strings in the DLL; owner's file 333 sections (236 `__EXT_`, 22 `__CM_`, 75 AC) |
| Lua | 136 input function names; 3 + 3 + 2 shipped scripts (12 files: 6 `assist.lua`-type 25,... B total, 2 `ffb.lua` 9,577 B); `ac_ffb_postprocess` and `ac_joypad_assist` environments 6,181 B / 15,149 B bytecode source in the DLL archive; 5 compiled plug-ins 17,6 KB |
| Python | 7 input functions of 213 |
| Plug-in executables | `AcTools.GamepadServer.exe` 89,592 B, `AcTools.Wooting.exe` 174,072 B, `AcTools.OpenRGB.exe` 213,728 B |
| Third-party code inside the DLL | Logitech SDK loader (51 names), TrackIR NPClient client (13 names), a hidapi copy, a JoyShock-style Switch / DualShock HID driver [interp] |
| Logging | owner's last session: 14 lines about devices (DirectInput start 27,936 ms, ready 28,587 ms) |

## 7. Port difficulty for rustyAC

**Overall M, with an S slice that matters on this PC and an L-XL slice that does not.**

What rustyAC already has (`crates/rustyac-game/src/input`, 5,249 lines): `controls.ini` reader and bindings
(`bindings.rs`, `ini.rs`), AC's `DICarControl` wheel class (`wheel.rs`: axes, buttons, shaping, vibration mix, centre
boost, min FF, skip steps), XInput pad + AC's `JoypadCarControl` (`pad.rs`: all 27 buttons, stick shaping, rumble),
`KeyboardCarControl` (`keyboard.rs`), DirectInput 8 reader with one constant-force effect behind `--ffb`
(`dinput.rs`), `MouseHider` (`mouse_hider.rs`), AC's FFB app and the pedals app (`rustyac-gui/app_ffb.rs`,
`app_pedals.rs`), `Car::getSteerFF` / `lastFF` in `rustyac-physics` (`chassis.rs`). Missing and named in the code:
H-pattern shifter, damper effect, `FFPostProcessor`, AC's soft lock, rev LEDs, mouse steering, a second key per
action, a clutch key, look-ahead on the AI line without a track.

| Slice | Difficulty | Why / what to reuse | How to prove |
|---|---|---|---|
| **Pad clutch** (`__EXT_KEYBOARD_CLUTCH`) + `__EXT_*` sections accepted and preserved in `controls.ini` | S | `ini.rs` already keeps whole sections; `CarControls.clutch` exists; helper 0xeceea0 is 453 B | run acs.exe (CSP on) with a scripted XInput shim, bind A to the clutch, compare `controls.clutch` per step in `car_oracle` |
| Ctrl+P unbound, turbo digits, D-pad release, `IMPROVED_MODIFIERS_HANDLING`, `IGNORE_ESCAPE_IN_REPLAYS` | S | key dispatch in the game loop | key-event scripts against the CSP-on and CSP-off game |
| Alternative bindings (`__ALT_<SECTION>_<n>`), `KEY_MODIFICATOR`, POV hats as buttons, `ENABLED` | S-M | extend `Bindings`; format fully visible in 0x5a7490 | binding tables from the owner's controls.ini plus synthetic hats |
| Device-order fix, placeholders, hot-plug, reconnect backoff | M | `dinput.rs` has enumeration; add the `[CONTROLLERS]` matching and the 128-poll backoff | unit tests from the log lines of `fix_devices_order` |
| Async polling / adaptive FFB / averaged skipped FFB / FFB offset | M | physics thread exists (`physics_thread.rs`); averaging is 10 lines | DI shim that logs `SetParameters` magnitudes (see below) |
| Custom soft lock, range compression, clean FFB, dirt vibration, real feel | M-L | formulas are in the shipped example Lua for the soft lock; the rest is in 0xcb87a0 (decompile first) | capture the DI constant-force magnitude sequence in AC+CSP vs rustyAC for scripted steering and a recorded physics tape; compare per step |
| Rumble: boost rate, skip bit, impulse triggers | S (rate, skip) / M (triggers) | `pad.rs set_vibrations` matches the AC part | `XInputSetState` shim logging values and steps |
| FFB / gamepad / mouse Lua scripts | L (needs the Lua runtime and `ac.getJoypadState`) | the shipped scripts are plain Lua; API is the 53-field struct | port the script in Lua run through LuaJIT vs recorded `StateJoypadData` tapes |
| DualSense / DualShock / Joy-Con (HID), light bar, adaptive triggers, haptics over WASAPI | L-XL | HID report formats are in no document here; JoyShock-like driver, per-model reports | needs the hardware; compare bytes written to the HID handle with a HID filter / USBPcap |
| TrackIR | M | NPClient64.dll is loaded by registry path; needs a window handle and a camera pipeline in rustyAC (no TrackIR in the port) | NP_GetData replay with a fake DLL, compare view offsets |
| Logitech SDK / TrueForce / LEDs | M-L | dynamic loading of a signed DLL; most of the surface is range and LEDs | log SDK calls with a proxy DLL |
| Steam Input block, OpenRGB, Wooting | L (low value) | helper processes | n/a |

Proof method that works for the whole FFB chain without a wheel: put a **proxy `dinput8.dll`** (or a virtual device
such as vJoy-FFB) in front of the game, record every `IDirectInputEffect::SetParameters` and `Start` of both builds
(acs.exe with CSP, rustyAC), and compare magnitude and damper per physics step for scripted steering inputs and the
same car on the same track; run once with `ENABLED=0` to separate stock AC from CSP. For the pad: an `XInput1_3`
proxy that feeds scripted states and logs `XInputSetState`. The input class bodies are cheap to verify; the ordering
(333 Hz poll, send every `FF_SKIP_STEPS + 1`) is what changes feel.

## 8. Open questions and not mapped

1. `InputDevice::InputDevice` real constructor 0x5fafc0 not read: cooperative level (exclusive or not), data
   format, range value, how `isNormalized` is chosen; `docs/atlas/input_ffb.md` open question 5 stays open.
2. What exactly runs on the `sendFF` bus (0x2690c88) and the `getSteerFF` bus (0x268ff60): soft lock, range
   compression, dirt vibration, script call, real feel; the registrars were not found (probably inside 0xcb87a0 and
   0xcb8280).
3. Meaning of the global byte 0x2695016 ("controls disabled": zeroed `CarControls`, `getAction` false) and of
   0x26a2c47 ("CSP UI owns the D-pad", written by 0xf46820, 0x10504d0, 0x1353940).
4. The horn toggle bit `[[car+0x28]+0x440]+0x11b`: which car feature it is.
5. The DeviceIoControl hook: whether it really adds Xbox One impulse triggers (strings and `WriteFile` only).
6. `AXIS_REMAP_THROTTLE` / `AXIS_REMAP_BRAKES`: which file and section (older CSP had `[AXIS_REMAP]` keys in
   `gamepad_fx.ini`, `prior/csp_survey.md` I2); the shipped section is empty.
7. `__EXT_KEYBOARD_CLUTCH`, `SHIFT_WITH_XBUTTONS`, `SHIFT_WITH_WHEEL`, `__EXT_CLUTCH_ALT`: which sections and
   what exactly (strings seen, bodies not).
8. Logitech: whether TrueForce streams are used, or only operating range and LEDs.
9. "Controller Tweaks" app: not installed here; presumably downloaded by the App Shelf; its keys (axis remap,
   steering shaping per gamepad, `ADVANCED_GAMEPADS_HAPTICS_%d`) live in `extension/state` and are per gamepad.
10. `AcTools.OpenRGB.exe` consumer; the Lua plug-ins `extra_buttons`, `extra_physics_buttons`, `stalling`,
    `reconnect_devices`, `dualsense_lights` are compiled bytecode and were only listed.
11. `X360Joypad::acquire` second mask (RVA 0x26a85c8): which callers write it (`ac.setXbox`, pad-as-mouse, scripts).
12. Online: server rule restricting input methods (0x10ab7e0) and the `[EXTRA_RULES]` side of FFB are only
    known by their messages.
13. The two registrations of `DirectInput::initJoys` (and `pollControls`) under registry gate records: which
    predicate selects which.

## Evidence

* `decomp_hooks/` (read in full): 5ae370, 5ae5c0, 5b3c10, 5b3d20, 5b4010, 5fab00, 5fb330, 5fc060, 5fc1a0, 5fc2b0,
  5fcf10, 1096dc0, 1096f10, 1097490, 1097740, 1097880, 1097900, 5a7260, 5a7450, 5a7490, 5a8000, 5a8130, 601b50,
  602430, 602b60, 602d10, 602eb0, 602f10, 69d7d0, 6bf260, 6bf270, 6063c0, 6067d0, 607490, 607680, 6236a0, 6238b0,
  112b780, 112bf90, 305980, 595800, 59fdb0, 10aabe0, 10ac580, 10ad370, 593f30, 593f70, 5a6700, 11beae0, 2db1f0.
* Tables: `hook_groups.tsv`, `csp_hook_table_raw.tsv`, `dll_strings.tsv`, `dll_func_strings.tsv`,
  `dll_func_imports.tsv`, `dll_funcs.tsv`, `dll_zips_all.json`; registration functions of the module-qualified
  names located by decoding the `lea rip` operands in `dwrite_copy.dll` (read only).
* Shipped files: `extension/config/{ffb_tweaks,gamepad_fx,g27_lights,general,small_tweaks,neck,freer_camera,
  data_manifest,data_controls_props}.ini`, `data_hotkeys_supported.txt`; `extension/lua/{joypad-assist,
  ffb-postprocess,mouse-steering}/*`; `extension/internal/lua-sdk/{ac_ffb_postprocess,ac_joypad_assist}`;
  `extension/internal/lua-shared/dualsense/*`; `extension/internal/python/functions.txt`; `apps/lua/ext_controls`.
* Owner: `Documents/Assetto Corsa/cfg/controls.ini`, `cfg/controllers/*.ini`, `cfg/extension/` listing,
  `logs/custom_shaders_patch.log` lines 26, 788-813, 835, 1252-1316.
* Earlier briefs: `attach.md` 1.7 / 1.8 / 2.2, `physics_car_hooks.md` (getSteerFF, pollControls, buses),
  `physics_documented.md` 3.1.5 / 3.1.6, `lua.md` 3.x, `prior/csp_survey.md` 4.2 (I1-I9).
* rustyAC side: `docs/atlas/input_ffb.md`, `docs/map/steering.md`, `crates/rustyac-game/src/input/*`,
  `mouse_hider.rs`, `crates/rustyac-gui/src/app_ffb.rs`.

## Decompile requests

1. 0x5fafc0 - InputDevice real constructor: cooperative level flags, data format, range, normalisation rule.
2. 0x5fce20 - FFB submit core: effect parameter update, damper rule, unit of the "-1" damper sentinel.
3. 0x5fc720, 0x5fcae0, 0x5fc400, 0x5fc3c5, 0x5fd0f0 - async polling / FFB thread, timing, flag setters.
4. 0x5fb530 - re-init / reconnect path.
5. 0xcb87a0 - ffb_tweaks module: where CUSTOM_SOFT_LOCK, RANGE_COMPRESSION, DIRT_VIBRATION, REAL_FEEL are applied.
6. 0xcb8280, 0x415870, 0x4154a0 - FFB script host and `ac.disableLowSpeedFFBReduction`.
7. 0xecf070, 0xecf1a0 - steering extrapolation (and holding handbrake?).
8. 0x1097d10, 0x1095970, 0x1096a60 - DICarControl record builder and its helpers.
9. 0x601b80, 0x601480, 0x601f50 - pad provider ctor, DualSense / DualShock choice, "New gamepad is available".
10. 0x6063f0, 0x606c40, 0xeceea0 - keyboard provider ctor, wrapper, clutch emulation.
11. 0xf81310 - DeviceIoControl hook: which IOCTL, what is written.
12. 0x6c86d0, 0xb70500, 0xf4b220, 0xf4b290 - `GetAsyncKeyState`, cursor hooks.
13. 0x5ae870, 0x5af860, 0x5b1df0, 0x5b0370, 0x5b1bb0 - device ranking, enumeration, order fix, monitor thread.
14. 0x13eb460, 0x6bc840, 0x6bd000 - DualSense HID enumeration and init.
15. 0x15ec400, 0x15ee97e, 0x16bb010 - Switch / DualShock HID driver.
16. 0x1db13a0, 0x1db8940 - DualSense haptics over audio.
17. 0x15cbda0, 0x15cb930, 0x1cd3cb0 - Logitech SDK use (what is sent through TrueForce, hardware lock value).
18. 0x1224300 - G27 module.
19. 0x112b810, 0x112b260, 0xbfb? (none) - TrackIR init / loader signature check.
20. 0xed06a0, 0xed0a70 - control files watcher and `[FF_EXPERIMENTAL]` reader.
21. 0xd0f480 - gamepad_fx loader (flag RVA 0x268a8cf and the others).
22. 0x1353940, 0xf464f0 - writers of the "CSP UI owns the D-pad" byte.
23. 0x134e3e0 - binding editor widgets (`__ALT_`, `REPEAT_PERIOD`, hold switch).
24. 0xd87a80 - the Python registrar containing `ext_getJoystick*`.
25. 0xe215e9 - `reconnectInputDevices`.
