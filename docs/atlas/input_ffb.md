# Input devices and force feedback hardware

Source: `re/decomp/` pseudo-C (folders `DirectInput`, `InputDevice`, `DICarControl`, `DICommandManager`,
`DIControlAxis`, `DIControlButton`, `DIControlInput`, `KeyboardCarControl`, `X360CarControl`, `JoypadManager`,
`KeyboardManager`, `CommandManager`, `MouseHider`, `FanatecUpdater`, `GearShiftShake`, `PhysicsDriveThread`),
`re/types/*.txt`, `re/index/compilands.tsv`, `tools/re_query.py` (callers / callees / strings / vtable), the
section names of a real `Documents\Assetto Corsa\cfg\controls.ini` and `system/cfg/fanatec.ini` (read-only; nothing
was modified). Builds on `docs/map/steering.md`, which already covers how physics computes the steering force.
"Confirmed" means read in the pseudo-C; "interpretation" is marked as such.

## 1. What it is

The physics car never talks to hardware. It owns one pointer, `Car::controlsProvider`, to an object implementing
`ICarControlsProvider` ("who drives"). Every physics step (3 ms) the car asks the provider for the pedal, steering
and button state, and hands back a steering force, a damper level, five vibration levels and the engine rpm.
There are three human providers, chosen once from `controls.ini [HEADER] INPUT_METHOD`: `DICarControl` (`WHEEL`,
any DirectInput device), `JoypadCarControl` (`X360`, XInput pad number 0) and `KeyboardCarControl` (`KEYBOARD`,
keys and optional mouse steering); `AI` selects the AI driver instead.
DirectInput devices are enumerated by a singleton `DirectInput` (`ksDI.lib`); each becomes an `InputDevice` that is
polled on the physics thread once per step, so wheel input has 333 Hz resolution. Force feedback is two DirectInput
effects per device, created once: a constant force (the steering torque) and a damper. The physics side supplies
one float in -1..1 and a damper gain; `DICarControl::sendFF` adds vibrations, minimum force, gains, optional
post-processing and soft lock, then `InputDevice::sendFF` updates the two effects.
The gamepad gets only rumble (two motors), the keyboard nothing. Keyboard and gamepad add their own steering
assistance (rate limits, speed-dependent lock, look-ahead on the AI line) inside the provider.
Everything that is not driving (camera keys, replay keys, toggles) goes through window key messages:
`KeyboardManager` dispatches them to listeners and `CommandManager` maps `controls.ini` sections to key codes.
Hardware extras: Logitech rev LEDs through a DirectInput escape, and Fanatec wheel LEDs / display through the
statically linked Fanatec SDK.

## 2. Main classes

| Class / unit | Source file (library) | Size | Role |
|---|---|---|---|
| `ICarControlsProvider` | (interface; see `docs/map/steering.md` 6) | 0x18 | Vtable: +0x08 `acquireControls(CarControls*, dt, CarControlsInput*)`, +0x10 `getAction`, +0x18 `sendFF(ff, damper, userGain)`, +0x20 `getName`, +0x28 `getFFGlobalGain`, +0x30 `isDeviceConnected`, +0x38 `onAutoShifterChanged`, +0x48 `setVibrations(VibrationDef*)`, +0x50 `setEngineRPM`. Fields `ffEnabled`, `ffFilter`, `isAutoclutchNeeded`, `useFakeUndersteerFF`, `keyboardEnabled`. |
| `CarControls` | physics | - | What a provider fills: `gas`, `brake`, `steer` (-1..1), `clutch`, `handBrake`, `gearUp`, `gearDn`, `requestedGearIndex` (H-shifter), `isShifterSupported`, `drs`, `kers`, brake balance, ABS / TC / turbo / engine-brake / MGU-K / MGU-H up-down flags. |
| `DirectInput` | DirectInput.obj (ksDI.lib) | 0x40 | Singleton: `IDirectInput8W* lpDI`, `devices` (vector of `InputDevice*`), list of device GUID + name. |
| `InputDevice` | InputDevice.obj (ksDI.lib) | 0x100 | One DirectInput device: `state` (`InputDeviceState`: 8 axes as floats, 4 POV hats, 128 buttons), `lpDirectInputDevice`, `lpFFConstantForce`, `lpFFDamp`, `isFFStarted`, `isDamperStarted`, `lastMagnitude`, `lastDampGain`, `isNormalized`, `lightCounter`, `name`. |
| `DIControlInput` / `DIControlAxis` / `DIControlButton` | own .obj files | - | One binding each. Axis: `[SECTION] JOY, AXLE, MIN, MAX`, value rescaled to 0..1 (pedals) or raw (steer), reports 0 until the axis has moved once. Button: `JOY, BUTTON`, plus `KEY` when `[ADVANCED] COMBINE_WITH_KEYBOARD_CONTROL` is on. |
| `DICarControl` | DICarControl.obj | 0xb50 | The wheel provider: about 30 bindings, steering shaping (`steerScale`, `linearity`, `steerLock`, `speedSensitivity`, `steerFilter`), `brakeGamma`, shifter (`isActive`, `joy`, 8 button indices), FF members (`ffGain`, `ffUpgrades`, `minFF`, `centerBoostGain`, `centerBoostRange`, `useSoftLock`, `ffPostProcessor`, `ffCounter`, `ffInterval`, `currentVibration`, `currentLock`). |
| `DICommandManager` | DICommandManager.obj | - | Turns the D-pad of the steering device into window key presses (`PostMessageA(hWnd, WM_KEYDOWN, key)`), so the wheel can drive menus / apps. |
| `JoypadManager`, `X360Joypad` | JoypadManager.obj (ksGame.lib) | 0x8 | XInput wrapper: `XInputGetState(0)`, sticks, triggers, button mask, `XInputSetState(0)`. |
| `JoypadCarControl`, `SecondaryJoypadButton` | X360CarControl.obj | 0x1a0 | Gamepad provider: `steerSpeed`, `steerGamma`, `steerFilter`, `deadZone`, `speedSensitivity`, `rumbleIntensity`, about 27 button bindings (each a pad button and/or a keyboard key). |
| `KeyboardCarControl` | KeyboardCarControl.obj | 0x120 | Keyboard / mouse provider: key codes per action, `steerSpeed`, `steerOppositeDirectionFact`, `steerResetFactor`, `gasPedalSpeed`, `mouseSteering`, `mouseSpeed`, `mouseAcceleratorBrake`, a `RaceEngineer` helper and a pointer to the AI spline. |
| `ExternalCarControl` | ExternalCarControl.obj | 0x1c8 | Only the type survives (a provider driven by function pointers `fInit`, `fAcquireControls`, `fOutput`, `fShutDown` from a DLL). The object file contains no real code in this build. |
| `KeyboardManager` | KeyboardManager.obj (ksGame.lib) | 0x30 | Receives key down / key press events from the render window and forwards them to `IKeyEventListener`s (or to the one with focus, e.g. a text box). |
| `CommandManager`, `CommandItem` | CommandManager.obj | 0x10 | Map from command name to key code, filled from `controls.ini` (`[ABS] KEY=`, `[TRACTION_CONTROL]`, `[ACTIVATE_AI]`, `[RESET_RACE]`, `[AUTO_SHIFTER]`, `[FFWD]`, `[REV]`, `[SLOWMO]`, `[HIDE_APPS]`, `[IDEAL_LINE]`, `[SHOW_DAMAGE]`, `[HIDE_DAMAGE]`, `[PREVIOUS_LAP]`, `[NEXT_LAP]`, `[PAUSE_REPLAY]`, `[START_REPLAY]`, `[DRIVER_NAMES]`, `[NEXT_CAR]`, `[PREVIOUS_CAR]`, `[PLAYER_CAR]`, `[MOUSE_STEERING]`, `[CYCLE_DESKTOP]` ...). |
| `MouseHider` | MouseHider.obj | 0x80 | Hides the cursor after `HIDE_INTERVAL` without movement; re-centres it for mouse steering. |
| `FanatecUpdater` | FanatecUpdater.obj | 0xb0 | Game object that drives a Fanatec wheel's rev LEDs and 3-digit display (speed, gear on change) through the Fanatec SDK interfaces `IFSDevice`, `IFSCmdLedRevs`, `IFSCmdLedSevenSegmentDisplayThreeDigits`. |
| `GearShiftShake` | GearShiftShake.obj | 0x128 | Not a force-feedback effect, despite the name: it animates the gear-lever mesh `SHIFT_HD` in the cockpit (blend between per-gear rotations plus an rpm-driven shake). Graphics only. |
| `FFPostProcessor` | FFPostProcessor.obj (ksPhysicsAC.lib) | 0x90 | Optional gamma or LUT curve on the final wheel force (`cfg/ff_post_process.ini`); owned by `DICarControl`. Described in `docs/map/steering.md`. |

## 3. Entry points

Creation
- `KeyboardManager::KeyboardManager` 0x140243200 and `JoypadManager::JoypadManager` 0x140243fa0 in `Game::Game`
  0x140241880.
- `DirectInput::singleton` 0x14023f290 -> `DirectInput::DirectInput` 0x14023e8b0 -> `DirectInput::initJoys`
  0x14023f030 (`DirectInput8Create`, device enumeration callback `EnumJoysticksCallbackDI` 0x14023eaa0) ->
  `InputDevice::InputDevice` 0x14023f2f0 per device (create, cooperative level on the game window, data format,
  then `InputDevice::initFF` 0x14023f580).
- `CarAvatar::initControls` 0x1400d6f30 reads `INPUT_METHOD` and creates `DICarControl::DICarControl`
  0x14007ddd0, `JoypadCarControl::JoypadCarControl` 0x1401d9280 or `KeyboardCarControl::KeyboardCarControl`
  0x14010f0e0 (which calls `KeyboardCarControl::readFromIni` 0x14010ff60), or an `AIDriver`, and installs it with
  `Car::setControllerProvider` 0x140275a70. `Sim::onKeyDown` 0x14019a940 can also create a `JoypadCarControl`.
- `DICommandManager::DICommandManager` 0x140081150 inside `PhysicsDriveThread::PhysicsDriveThread`, which also
  reads `INPUT_METHOD` to decide `useDirectInput`.
- `CommandManager::CommandManager` 0x1400e8b70, `MouseHider::MouseHider` 0x140114850 and
  `FanatecUpdater::FanatecUpdater` 0x140099f70 in `Sim::Sim` 0x140192070; `GearShiftShake::GearShiftShake`
  0x1401048f0 in `CarAvatar::initCommonPostPhysics` 0x1400d6190.

Physics thread, every 3 ms step (confirmed in `PhysicsDriveThread::run` 0x140123150 and `::step` 0x140123560)
1. `DirectInput::poll` 0x14023f160 -> `InputDevice::poll` 0x14023f780 for each device (`Poll`,
   `GetDeviceState` of a `DIJOYSTATE2`; axes scaled by 0.0001 when the device is "normalized"; re-acquires on
   input loss), then `DICommandManager::step` 0x140081730. This also runs while paused.
2. `PhysicsEngine::step` -> `Car::pollControls` 0x140274e70 -> provider `acquireControls` (+0x08), then
   `setVibrations` (+0x48) and `setEngineRPM` (+0x50).
3. Later in the same step `Car::onTyresStepCompleted` 0x140274cd0 computes `Car::lastFF` and calls provider
   `sendFF(lastFF, damper, userFFGain)` (+0x18). This is the only reader of the physics force on the hardware
   side; `Car::getFinalFF` 0x140270960 only reports it to shared memory.
- Main thread: `PhysicsAvatar::render` 0x140122320 calls `DirectInput::forceFF(0)` 0x14023ef90 while paused (zero
  force on every device). `FanatecUpdater::update` 0x14009a610, `GearShiftShake::update` 0x140104ca0 and
  `MouseHider::update` 0x140114ed0 are ordinary per-frame game-object updates.

Wheel input shaping, `DICarControl::acquireControls` 0x14007fe70 (confirmed)
```
steer = axis * SCALE
if STEER_GAMMA != 1:  steer = sign(steer) * |steer|^STEER_GAMMA
if car steer lock < LOCK/2:  steer *= (LOCK/2) / car steer lock      # wheel rotation matched to the car
if SPEED_SENSITIVITY != 0:  steer /= (1 + SPEED_SENSITIVITY * speed)
if STEER_FILTER > 0:  steer = previous + (steer - previous) * clamp(STEER_FILTER * dt)
currentLock = steer;  controls.steer = clamp(steer, -1, 1)
gas    = 0 below 0.02, else clamp(axis)
brake  = clamp(axis with the same 0.02 dead zone) ^ [BRAKES] GAMMA
clutch = 1 - axis (0.02 dead zone);  handBrake = clamp(button or key + axis)
```
Gear paddles are debounced with `Trigger::keepSteady` 0x14023b3c0 (`[STEER] DEBOUNCING_MS`). H-pattern: when
`[SHIFTER] ACTIVE` and the car supports it (`DICarControl::initShifter` 0x1400807f0 reads `JOY`, `GEAR_1..7`,
`GEAR_R`), `requestedGearIndex` = 0 for the reverse button, n + 1 for gear n, 1 (neutral) when no button is held;
otherwise -1 and the sequential `gearUp` / `gearDn` flags are used. Every button also accepts its keyboard key
through `DIControlButton::getKeyboardValue` 0x140081c50.

Force feedback output (confirmed)
- `DICarControl::setVibrations` 0x140081010: `currentVibration = clamp(ROAD * gforce + CURBS * curbs + SLIPS * slips
  + ABS * abs, -1, 1)` with the gains of `[FF_ENHANCEMENT]`. The five raw levels are generated by physics in
  `Car::pollControls` (kerb saw-tooth from the surface, road sine from lateral g, slip sine, ABS pulse; formulas in
  `docs/map/steering.md` 5.7); the input side only mixes them.
- `DICarControl::sendFF` 0x140080e40, in order: centre boost (`CENTER_BOOST_GAIN`, `CENTER_BOOST_RANGE`); minimum
  force: if `|ff| < MIN_FF * clamp((speed km/h - 1) * 0.2)` the force is raised to that level with its sign; skip
  `ffInterval` steps between sends (`FF_SKIP_STEPS`, so the update rate is 333 Hz / (skip + 1)); zero if
  `ffEnabled` is off; then `out = (ff + currentVibration) * FF_GAIN * userGain`, optional
  `FFPostProcessor::getProcessedFF` 0x140279a80, and `InputDevice::sendFF`. With soft lock on and the wheel past
  the car's lock (`|currentLock| > 1`) it sends full force towards centre with damper 1 instead.
- `InputDevice::initFF`: `SetProperty(DIPROP_AUTOCENTER, off)`, then `CreateEffect(GUID_ConstantForce)` and
  `CreateEffect(GUID_Damper)`: one axis, cartesian, infinite duration, gain 10000. No spring, friction or periodic
  effects are created: kerbs and slips are sums inside the constant force.
- `InputDevice::sendFF` 0x14023f9b0: magnitude = clamp(ff * 10000, -10000, 10000) via `SetParameters` (with
  `DIEP_START` the first time); the damper's coefficients = damper * 10000 are updated only when the value changed.
- `DICarControl::getFFGlobalGain` 0x140080790 returns `ffGain` (used for the shared-memory "final FF").
- Gamepad: `JoypadCarControl::setVibrations` 0x1401dc3c0 sets the left motor to `RUMBLE_INTENSITY` while the kerb
  level exceeds 0.05 (legacy mode: engine level * 0.15 otherwise) and the right motor to `|slips| *
  RUMBLE_INTENSITY`; `JoypadCarControl::sendFF` 0x1401dc370 forwards them to `X360Joypad::setVibrations`
  0x140244110 every 10th step (about 33 Hz).
- Logitech: `DICarControl::setEngineRPM` 0x140081000 and `InputDevice::setG27Leds` 0x14023fb80 send the rev-LED
  escape (`IDirectInputDevice8::Escape` with rpm, first-LED rpm, red-line rpm) every 50th call. No Thrustmaster
  code exists; the only device special case is the "SideWinder Force Feedback Wheel (USB)", whose `ffInterval` is
  forced to 4.

Keyboard and gamepad assistance (skimmed)
- `KeyboardCarControl::acquireControls` 0x14010f2e0 uses `GetAsyncKeyState`. Steering:
  `getKeyboardSteering` 0x14010faa0 limits the target to `pi / (1 + 0.5 * speed)` and moves towards it in
  `stepSteer` 0x140111650 / `steeringMovement` 0x140111360, which also look ahead on the AI best-lap spline
  (`getCarPositionOnSpline` 0x14010f8e0, `getPointAtDistance` 0x14010fc50). Throttle:
  `computeGasCoefficient` 0x14010f6f0 ramps the pedal and backs off (not below 0.65) while a driven tyre's
  normalised slip exceeds 0.99 (2.0 above 100 km/h). Braking uses `RaceEngineer::getOptimalBrake`. Mouse:
  `getMouseSteering` 0x14010fb70 integrates cursor offset from the window centre divided by `MOUSE_SPEED`.
- `JoypadCarControl::acquireControls` 0x1401dbce0: stick -> `getAxisValue` 0x1401dc250 (dead zone, gamma), a
  speed-dependent maximum steer (`SPEED_SENSITIVITY`), rate limit `STEER_SPEED`, filter `STEER_FILTER`; triggers
  are gas and brake.

## 4. What it reads from disk

| Path | Read by | Sections / keys |
|---|---|---|
| `cfg/controls.ini` (Documents) | `CarAvatar::initControls`, `PhysicsDriveThread` | `[HEADER] INPUT_METHOD` = `WHEEL`, `X360`, `KEYBOARD`, `AI` (`EXTERNAL` is also compared in `PhysicsDriveThread`). |
| same | `DICarControl::DICarControl`, `DIControlAxis::load` 0x140081990, `DIControlButton::load` 0x140081cc0 | Axes `[STEER]`, `[THROTTLE]`, `[BRAKES]`, `[CLUTCH]`, `[HANDBRAKE]`: `JOY, AXLE, MIN, MAX`. `[STEER]` also `SCALE, LOCK, STEER_GAMMA, STEER_FILTER, SPEED_SENSITIVITY, FF_GAIN, FILTER_FF, DEBOUNCING_MS`; `[BRAKES] GAMMA`. Buttons (`JOY, BUTTON, KEY`): `[GEARUP]`, `[GEARDN]`, `[GLANCELEFT]`, `[GLANCERIGHT]`, `[GLANCEBACK]`, `[BALANCEUP]`, `[BALANCEDN]`, `[KERS]`, `[DRS]`, `[ABSUP]`, `[ABSDN]`, `[TCUP]`, `[TCDN]`, `[TURBOUP]`, `[TURBODN]`, `[ENGINE_BRAKE_UP]`, `[ENGINE_BRAKE_DN]`, `[MGUK_DELIVERY_UP/DN]`, `[MGUK_RECOVERY_UP/DN]`, `[MGUH_MODE]`, `[ACTION_HEADLIGHTS]`, `[ACTION_HEADLIGHTS_FLASH]`, `[ACTION_HORN]`, `[ACTION_CHANGE_CAMERA]`. FF: `[FF_ENHANCEMENT] CURBS, ROAD, SLIPS, ABS`; `[FF_ENHANCEMENT_2] UNDERSTEER`; `[FF_TWEAKS] MIN_FF, CENTER_BOOST_GAIN, CENTER_BOOST_RANGE`; `[FF_SKIP_STEPS] VALUE`; `[SHIFTER] ACTIVE, JOY, GEAR_1..GEAR_7, GEAR_R`; `[ADVANCED] COMBINE_WITH_KEYBOARD_CONTROL, DPAD_INDEX_OVERRIDE`. |
| same | `KeyboardCarControl::readFromIni` | `[KEYBOARD] STEERING_SPEED, STEERING_OPPOSITE_DIRECTION_SPEED, STEER_RESET_SPEED, MOUSE_STEER, MOUSE_SPEED, MOUSE_ACCELERATOR_BRAKE`; `[GAS]`, `[BRAKE]`, `[LEFT]`, `[RIGHT]` and the action sections above with `KEY`. The strings `STEER_GAIN` and `LOOKAHEAD_POINTS` are also referenced. |
| same | `JoypadCarControl::JoypadCarControl` | `[X360] STEER_THUMB` (`LEFT` / other), `RUMBLE_INTENSITY, STEER_SPEED, STEER_GAMMA, STEER_FILTER, STEER_DEADZONE, SPEED_SENSITIVITY`; action sections with `XBOXBUTTON` (`A, B, X, Y, LSHOULDER, RSHOULDER, DPAD_*, LEFT_THUMB, RIGHT_THUMB, LTHUMB_PRESS, RTHUMB_PRESS, START, BACK`, `-1` = none) and `KEY`. |
| same | `CommandManager::CommandManager` | One section per command with `KEY` (list in section 2). |
| `system/cfg/assetto_corsa.ini` | `DICarControl::DICarControl`, `JoypadCarControl::JoypadCarControl` | `[FORCE_FEEDBACK] FF_SKIP_STEPS` (overridden by `controls.ini [FF_SKIP_STEPS] VALUE`), `[SOFT_LOCK] ENABLED`, `[GAMEPAD] USE_LEGACY_CODE`. |
| `cfg/ff_post_process.ini` (Documents) | `FFPostProcessor::FFPostProcessor` 0x140279560 | `[HEADER] ENABLED, TYPE`, `[GAMMA] VALUE` (a LUT type also exists; see `docs/map/steering.md`). |
| `system/cfg/fanatec.ini` | `FanatecUpdater::FanatecUpdater` | `[SETTINGS] ENABLED, BLINK_MAX_TIME, MESSAGE_MAX_TIME, GEAR_MAX_TIME, SHOW_NEXT_GEAR`. |
| `system/cfg/mouse_hider.ini` | `MouseHider::MouseHider` | `[SETTINGS] HIDE_INTERVAL, RESET_POSITION`. |
| car model `.kn5` | `GearShiftShake::GearShiftShake` | Looks for the node `SHIFT_HD`. |

## 5. Size

`python tools/re_query.py size DirectInput InputDevice DICarControl DICommandManager DIControlAxis DIControlButton
DIControlInput KeyboardCarControl X360CarControl JoypadManager KeyboardManager ExternalCarControl CommandManager
MouseHider FanatecUpdater GearShiftShake`

| Object file | Functions | Code bytes | | Object file | Functions | Code bytes |
|---|---|---|---|---|---|---|
| DirectInput.obj (ksDI.lib) | 11 | 2,976 | | X360CarControl.obj | 12 | 16,611 |
| InputDevice.obj (ksDI.lib) | 6 | 2,350 | | JoypadManager.obj (ksGame.lib) | 8 | 362 |
| DICarControl.obj | 16 | 15,928 | | KeyboardManager.obj (ksGame.lib) | 14 | 4,085 |
| DICommandManager.obj | 2 | 1,883 | | ExternalCarControl.obj | 7 | 469 |
| DIControlAxis.obj | 5 | 840 | | CommandManager.obj | 3 | 12,457 |
| DIControlButton.obj | 6 | 972 | | MouseHider.obj | 17 | 2,694 |
| DIControlInput.obj | 3 | 335 | | FanatecUpdater.obj | 4 | 2,953 |
| KeyboardCarControl.obj | 23 | 11,778 | | GearShiftShake.obj | 4 | 1,808 |

**Total: 141 hand-written functions, 78,501 code bytes** (16 object files).

Not counted here:
- `EndorFanatecSdk64_VS2013_MD.lib`: 42 object files, 373 symbols, 51,066 code bytes, third-party (no pseudo-C in
  `re/decomp`). Only `FanatecUpdater` uses it.
- `FFBForm.obj` (34 functions, 14,238 bytes), `KeyboardControlsDebug.obj` (13, 7,321) and `X360ControllerDebug.obj`
  (24, 9,196) are SystemApps.lib forms (FF clipping meter and input debug apps): UI area.
- `FFPostProcessor.obj` (ksPhysicsAC.lib) and `Car::pollControls` / `Car::onTyresStepCompleted`: car physics
  (`docs/map/steering.md`). `Trigger.obj` (ksCommon.lib): shared helper. `ACControl.obj` (acSDK.lib) is a GUI
  control for Python apps, not an input device. `LockControlsTimeAvatar.obj` belongs to session logic.

## 6. Port difficulty

**M.** Little code and simple maths, but it is all Windows API work that has to be replaced, and feel depends on
details (333 Hz polling on the physics thread, the exact FF chain).
- Depends on: the physics `Car` (provider interface, `CarControls`, `CarControlsInput` with speed and steer lock),
  ini reader, the window (cooperative level, key messages), the AI spline and `RaceEngineer` (keyboard assistance).
- Port 1:1 (small, worth being exact): the provider interface, `DICarControl::acquireControls`, `setVibrations`,
  `sendFF`, the keyboard and gamepad steering assistance, the `controls.ini` format.
- Replace with crates: gamepad -> `gilrs` (or `rusty-xinput`); keyboard / mouse -> `winit` events; wheels with force
  feedback -> DirectInput through the `windows` crate on Windows (constant force + damper is all that is needed) or
  SDL's haptic API (`sdl2` / `sdl3` crates) for a cross-platform path. No pure-Rust crate covers DirectInput FF well.
- Third-party SDKs: Fanatec (Endor) SDK for LEDs and display - optional, can be dropped or replaced by HID writes;
  Logitech LEDs are a plain DirectInput escape.
- `GearShiftShake` belongs with car graphics when ported.

## 7. Open questions

- Centre boost: the pseudo-C reads `ff = ff * (1 - |steer| / range) * gain` inside the range, which would reduce the
  force for gains below 1 (same doubt as `docs/map/steering.md` open question 6). Check the disassembly.
- `[FF_ENHANCEMENT_2] UNDERSTEER` and `[STEER] FILTER_FF` are read in the constructor; they presumably set
  `useFakeUndersteerFF` and `ffFilter` of the provider (used by physics), not confirmed line by line.
- `DICommandManager` builds exactly four commands from the POV hat of the steering device (or
  `DPAD_INDEX_OVERRIDE`); which key codes they post was not decoded (interpretation: the arrow keys).
- `INPUT_METHOD=EXTERNAL` is still compared in `PhysicsDriveThread`, but `CarAvatar::initControls` has no such
  branch and `ExternalCarControl.obj` is empty: treated as a removed feature.
- `isNormalized` (axes scaled by 0.0001) and the data format / axis range set in `InputDevice::InputDevice` were
  not decoded.
- `KeyboardCarControl::stepSteer` / `steeringMovement` and the gamepad speed-sensitivity curve were only skimmed;
  the constants need a proper read before porting.
- Only XInput user index 0 is used; whether DirectInput-only gamepads work through `WHEEL` mode was not checked.
- Choice made: the three SystemApps debug forms and the Fanatec SDK are described but not counted in this area.
