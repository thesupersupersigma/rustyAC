//! DirectInput 8: the game controllers Windows lists (wheels, pedals, other pads), read the
//! way AC's `DirectInput` / `InputDevice` read them (`re/scratch/task11/spec_wheel_ffb.md`
//! sections 1 and 2), and one constant-force effect for force feedback.
//!
//! Differences from AC, on purpose: devices are opened non-exclusive (AC takes every
//! controller exclusively), an Xbox pad's DirectInput twin is listed but not opened (the pad
//! is read through XInput), a failed read keeps the last state (AC reads zeros), and force
//! feedback exists only with `--ffb`, capped far below the wheel's strength.

use std::ffi::c_void;

use windows::core::{Interface, GUID};
use windows::Win32::Devices::HumanInterfaceDevice::*;
use windows::Win32::Foundation::{HINSTANCE, HWND};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow;

use super::wheel::WheelDevice;

/// `InputDeviceState`: the 8 axes as -1..1, the POV hats, the buttons.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiState {
    /// lX, lY, lZ, lRx, lRy, lRz, slider 0, slider 1 (`AXLE` 0..7).
    pub axes: [f32; 8],
    pub povs: [u32; 4],
    pub buttons: [u8; 128],
}

impl Default for DiState {
    fn default() -> DiState {
        DiState { axes: [0.0; 8], povs: [0; 4], buttons: [0; 128] }
    }
}

/// `InputDevice::poll`'s scaling: the device's range is set to -10000..10000.
pub fn axis_value(raw: i32) -> f32 {
    raw as f32 * 0.0001
}

/// Force feedback is never stronger than this share of the wheel's own maximum
/// (`DIPROP_FFGAIN`, out of 10000).
pub const FF_DEVICE_GAIN: u32 = 3000;

pub struct DiDevice {
    /// AC's `JOY` number: the place in the enumeration.
    pub index: usize,
    pub name: String,
    /// An XInput pad's twin: listed, never opened.
    pub is_xinput: bool,
    pub has_ff: bool,
    /// A steering wheel (`DI8DEVTYPE_DRIVING`).
    pub is_wheel: bool,
    device: Option<IDirectInputDevice8W>,
    pub state: DiState,
    effect: Option<IDirectInputEffect>,
    ff_started: bool,
    last_magnitude: i32,
}

pub struct DirectInput {
    _di: IDirectInput8W,
    pub devices: Vec<DiDevice>,
    window: HWND,
    /// Kept alive: the devices point into it.
    _format: Box<[DIOBJECTDATAFORMAT]>,
}

static AXIS_GUIDS: [GUID; 7] = [GUID_XAxis, GUID_YAxis, GUID_ZAxis, GUID_RxAxis, GUID_RyAxis, GUID_RzAxis, GUID_Slider];
static POV_GUID: GUID = GUID_POV;
/// `DIDFT_OPTIONAL`: the device need not have the object.
const OPTIONAL: u32 = 0x8000_0000;

/// The data format of `DIJOYSTATE2` (`c_dfDIJoystick2`, which is not in any DLL): the 8
/// position axes, 4 hats and 128 buttons. The velocity and force entries of the original
/// table are left out; nothing reads them.
fn joystick_format() -> Box<[DIOBJECTDATAFORMAT]> {
    let mut objects = Vec::with_capacity(140);
    for (k, offset) in [0u32, 4, 8, 12, 16, 20, 24, 28].into_iter().enumerate() {
        objects.push(DIOBJECTDATAFORMAT {
            pguid: &AXIS_GUIDS[k.min(6)],
            dwOfs: offset,
            dwType: OPTIONAL | DIDFT_AXIS | DIDFT_ANYINSTANCE,
            dwFlags: DIDOI_ASPECTPOSITION,
        });
    }
    for offset in [32u32, 36, 40, 44] {
        objects.push(DIOBJECTDATAFORMAT { pguid: &POV_GUID, dwOfs: offset, dwType: OPTIONAL | DIDFT_POV | DIDFT_ANYINSTANCE, dwFlags: 0 });
    }
    for button in 0..128u32 {
        objects.push(DIOBJECTDATAFORMAT { pguid: std::ptr::null(), dwOfs: 48 + button, dwType: OPTIONAL | DIDFT_BUTTON | DIDFT_ANYINSTANCE, dwFlags: 0 });
    }
    objects.into_boxed_slice()
}

/// `DIPROP_RANGE` and friends are numbers dressed as pointers (`MAKEDIPROP`).
fn prop(number: usize) -> *const GUID {
    number as *const GUID
}

struct Found {
    instance: GUID,
    name: String,
    dev_type: u32,
}

unsafe extern "system" fn enumerate(instance: *mut DIDEVICEINSTANCEW, context: *mut c_void) -> windows::core::BOOL {
    // SAFETY: DirectInput hands a valid instance; the context is the `Vec` passed to `EnumDevices`.
    unsafe {
        let found = &mut *(context as *mut Vec<Found>);
        let instance = &*instance;
        let length = instance.tszProductName.iter().position(|c| *c == 0).unwrap_or(260);
        found.push(Found { instance: instance.guidInstance, name: String::from_utf16_lossy(&instance.tszProductName[..length]), dev_type: instance.dwDevType });
    }
    true.into()
}

impl DirectInput {
    /// Lists the attached game controllers in AC's order and opens those that are not XInput
    /// pads. `window` is the game's window (0: none, the desktop stands in).
    pub fn open(window: isize) -> Result<DirectInput, String> {
        // SAFETY: COM calls with valid arguments; out pointers are to locals; the data
        // format outlives the devices (it is stored next to them).
        unsafe {
            let instance: HINSTANCE = GetModuleHandleW(None).map_err(|e| e.to_string())?.into();
            let mut raw: *mut c_void = std::ptr::null_mut();
            DirectInput8Create(instance, DIRECTINPUT_VERSION, &IDirectInput8W::IID, &mut raw, None).map_err(|e| format!("DirectInput: {e}"))?;
            let di = IDirectInput8W::from_raw(raw);
            let mut found: Vec<Found> = Vec::new();
            di.EnumDevices(DI8DEVCLASS_GAMECTRL, Some(enumerate), &mut found as *mut _ as *mut c_void, DIEDFL_ATTACHEDONLY)
                .map_err(|e| format!("DirectInput device list: {e}"))?;
            let window = if window == 0 { GetDesktopWindow() } else { HWND(window as *mut c_void) };
            let mut format = joystick_format();
            let mut devices = Vec::new();
            for (index, f) in found.into_iter().enumerate() {
                let mut entry = DiDevice {
                    index,
                    name: f.name,
                    is_xinput: false,
                    has_ff: false,
                    is_wheel: f.dev_type & 0xff == DI8DEVTYPE_DRIVING,
                    device: None,
                    state: DiState::default(),
                    effect: None,
                    ff_started: false,
                    last_magnitude: 0,
                };
                let mut device = None;
                if di.CreateDevice(&f.instance, &mut device, None).is_ok() {
                    if let Some(device) = device {
                        // an XInput pad's device path holds "IG_"
                        let mut path = DIPROPGUIDANDPATH {
                            diph: DIPROPHEADER { dwSize: std::mem::size_of::<DIPROPGUIDANDPATH>() as u32, dwHeaderSize: 16, dwObj: 0, dwHow: DIPH_DEVICE },
                            ..Default::default()
                        };
                        if device.GetProperty(prop(12), &mut path.diph).is_ok() {
                            let length = path.wszPath.iter().position(|c| *c == 0).unwrap_or(260);
                            entry.is_xinput = String::from_utf16_lossy(&path.wszPath[..length]).to_ascii_uppercase().contains("IG_");
                        }
                        if !entry.is_xinput {
                            let mut data_format = DIDATAFORMAT {
                                dwSize: std::mem::size_of::<DIDATAFORMAT>() as u32,
                                dwObjSize: std::mem::size_of::<DIOBJECTDATAFORMAT>() as u32,
                                dwFlags: DIDF_ABSAXIS,
                                dwDataSize: std::mem::size_of::<DIJOYSTATE2>() as u32,
                                dwNumObjs: format.len() as u32,
                                rgodf: format.as_mut_ptr(),
                            };
                            let ready = device.SetDataFormat(&mut data_format).is_ok()
                                && device.SetCooperativeLevel(window, DISCL_BACKGROUND | DISCL_NONEXCLUSIVE).is_ok();
                            if ready {
                                let mut range = DIPROPRANGE {
                                    diph: DIPROPHEADER { dwSize: std::mem::size_of::<DIPROPRANGE>() as u32, dwHeaderSize: 16, dwObj: 0, dwHow: DIPH_DEVICE },
                                    lMin: -10000,
                                    lMax: 10000,
                                };
                                let _ = device.SetProperty(prop(4), &mut range.diph);
                                let mut caps = DIDEVCAPS { dwSize: std::mem::size_of::<DIDEVCAPS>() as u32, ..Default::default() };
                                if device.GetCapabilities(&mut caps).is_ok() {
                                    entry.has_ff = caps.dwFlags & DIDC_FORCEFEEDBACK != 0;
                                }
                                // may fail now; the poll tries again
                                let _ = device.Acquire();
                                entry.device = Some(device);
                            }
                        }
                    }
                }
                devices.push(entry);
            }
            Ok(DirectInput { _di: di, devices, window, _format: format })
        }
    }

    /// `InputDevice::poll` for every open device: a device that was lost is acquired again
    /// and keeps its last state for this step.
    pub fn poll(&mut self) {
        for entry in &mut self.devices {
            let Some(device) = &entry.device else { continue };
            // SAFETY: COM calls on a live device; the state struct is the format's size.
            unsafe {
                if let Err(e) = device.Poll() {
                    if e.code() == DIERR_INPUTLOST || e.code() == DIERR_NOTACQUIRED {
                        let _ = device.Acquire();
                        entry.ff_started = false;
                        continue;
                    }
                }
                let mut js = DIJOYSTATE2::default();
                if device.GetDeviceState(std::mem::size_of::<DIJOYSTATE2>() as u32, &mut js as *mut _ as *mut c_void).is_ok() {
                    let raw = [js.lX, js.lY, js.lZ, js.lRx, js.lRy, js.lRz, js.rglSlider[0], js.rglSlider[1]];
                    entry.state = DiState { axes: raw.map(axis_value), povs: js.rgdwPOV, buttons: js.rgbButtons };
                }
            }
        }
    }

    /// The state of `JOY` number `joy`, if that device is open.
    pub fn state(&self, joy: i32) -> Option<&DiState> {
        let entry = self.devices.get(usize::try_from(joy).ok()?)?;
        entry.device.as_ref().map(|_| &entry.state)
    }

    /// Prepares force feedback on a device: exclusive access (DirectInput insists), the
    /// wheel's own centring spring off, the strength capped, one constant force as AC makes
    /// it. Returns why not, if it cannot be done.
    pub fn enable_ff(&mut self, joy: i32) -> Result<(), String> {
        let window = self.window;
        let entry = usize::try_from(joy).ok().and_then(|k| self.devices.get_mut(k)).ok_or("no such device")?;
        let device = entry.device.as_ref().ok_or("the device is not open")?;
        if !entry.has_ff {
            return Err(format!("{} has no force feedback", entry.name));
        }
        // SAFETY: COM calls on a live device; the effect's arrays are locals that outlive the call.
        unsafe {
            let _ = device.Unacquire();
            device
                .SetCooperativeLevel(window, DISCL_BACKGROUND | DISCL_EXCLUSIVE)
                .map_err(|e| format!("exclusive access to {} (is another game using it?): {e}", entry.name))?;
            let header = DIPROPHEADER { dwSize: std::mem::size_of::<DIPROPDWORD>() as u32, dwHeaderSize: 16, dwObj: 0, dwHow: DIPH_DEVICE };
            let mut autocentre = DIPROPDWORD { diph: header, dwData: 0 };
            let _ = device.SetProperty(prop(9), &mut autocentre.diph);
            let mut gain = DIPROPDWORD { diph: header, dwData: FF_DEVICE_GAIN };
            device.SetProperty(prop(7), &mut gain.diph).map_err(|e| format!("the strength cap was refused, no force feedback: {e}"))?;
            device.Acquire().map_err(|e| format!("acquiring {}: {e}", entry.name))?;
            let mut axes = [0u32];
            let mut direction = [0i32, 0];
            let mut force = DICONSTANTFORCE { lMagnitude: 0 };
            let mut effect_desc = DIEFFECT {
                dwSize: std::mem::size_of::<DIEFFECT>() as u32,
                dwFlags: DIEFF_CARTESIAN | DIEFF_OBJECTOFFSETS,
                dwDuration: u32::MAX,
                dwGain: 10000,
                dwTriggerButton: DIEB_NOTRIGGER,
                dwTriggerRepeatInterval: u32::MAX,
                cAxes: 1,
                rgdwAxes: axes.as_mut_ptr(),
                rglDirection: direction.as_mut_ptr(),
                cbTypeSpecificParams: std::mem::size_of::<DICONSTANTFORCE>() as u32,
                lpvTypeSpecificParams: &mut force as *mut _ as *mut c_void,
                ..Default::default()
            };
            let mut effect = None;
            device.CreateEffect(&GUID_ConstantForce, &mut effect_desc, &mut effect, None).map_err(|e| format!("the force effect: {e}"))?;
            entry.effect = effect;
            entry.ff_started = false;
        }
        Ok(())
    }

    /// `InputDevice::sendFF` (the constant force only): -1..1 of the capped strength. A
    /// number that is not one sends no force (AC would send full force one way).
    pub fn send_ff(&mut self, joy: i32, ff: f32) {
        let Some(entry) = usize::try_from(joy).ok().and_then(|k| self.devices.get_mut(k)) else { return };
        let Some(effect) = &entry.effect else { return };
        let magnitude = magnitude(ff);
        if entry.ff_started && magnitude == entry.last_magnitude {
            return;
        }
        let mut axes = [0u32];
        let mut direction = [1i32, 1];
        let mut force = DICONSTANTFORCE { lMagnitude: magnitude };
        let mut effect_desc = DIEFFECT {
            dwSize: std::mem::size_of::<DIEFFECT>() as u32,
            dwFlags: DIEFF_CARTESIAN | DIEFF_OBJECTOFFSETS,
            dwDuration: u32::MAX,
            dwGain: 10000,
            dwTriggerButton: DIEB_NOTRIGGER,
            dwTriggerRepeatInterval: u32::MAX,
            cAxes: 1,
            rgdwAxes: axes.as_mut_ptr(),
            rglDirection: direction.as_mut_ptr(),
            cbTypeSpecificParams: std::mem::size_of::<DICONSTANTFORCE>() as u32,
            lpvTypeSpecificParams: &mut force as *mut _ as *mut c_void,
            ..Default::default()
        };
        let flags = if entry.ff_started { DIEP_TYPESPECIFICPARAMS } else { DIEP_START | DIEP_TYPESPECIFICPARAMS | DIEP_DIRECTION };
        // SAFETY: the effect is alive; the arrays outlive the call.
        if unsafe { effect.SetParameters(&mut effect_desc, flags) }.is_ok() {
            entry.ff_started = true;
            entry.last_magnitude = magnitude;
        }
    }
}

/// `InputDevice::sendFF`'s magnitude: the force in ten-thousandths, within +-10000; not a
/// number gives 0.
pub fn magnitude(ff: f32) -> i32 {
    if ff.is_nan() {
        return 0;
    }
    ((ff * 10000.0) as i32).clamp(-10000, 10000)
}

impl Drop for DirectInput {
    fn drop(&mut self) {
        for entry in &mut self.devices {
            // SAFETY: COM calls on objects this struct owns.
            unsafe {
                if let Some(effect) = entry.effect.take() {
                    let _ = effect.Stop();
                }
                if let Some(device) = entry.device.take() {
                    let _ = device.Unacquire();
                }
            }
        }
    }
}

/// The DirectInput part of the device list, for the console.
pub fn describe_devices(wheel: &Option<WheelDevice>) -> String {
    let Some(wheel) = wheel else {
        return "  DirectInput: could not be opened\n".to_string();
    };
    let mut out = String::new();
    if wheel.di.devices.is_empty() {
        out.push_str("  no DirectInput game controller attached\n");
    }
    for d in &wheel.di.devices {
        let what = if d.is_xinput {
            "an Xbox pad's DirectInput twin: read through XInput instead".to_string()
        } else {
            format!(
                "{}{}{}",
                if d.is_wheel { "steering wheel" } else { "game controller" },
                if d.has_ff { ", force feedback" } else { "" },
                if wheel.steer_joy() == Some(d.index as i32) { ", steers" } else { "" }
            )
        };
        out.push_str(&format!("  DirectInput JOY={} \"{}\" ({what})\n", d.index, d.name));
    }
    for note in &wheel.notes {
        out.push_str(&format!("  {note}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axes_and_forces_scale_like_inputdevice() {
        // spec 9.A
        for (raw, bits) in [(10000, 0x3f800000u32), (-10000, 0xbf800000), (5000, 0x3f000000), (-2500, 0xbe800000), (1, 0x38d1b717), (3333, 0x3eaaa64c), (9999, 0x3f7ff972), (200, 0x3ca3d70a)] {
            assert_eq!(axis_value(raw).to_bits(), bits, "{raw}");
        }
        // spec 9.I.34, but not-a-number is no force
        assert_eq!((magnitude(1.5), magnitude(-2.0), magnitude(-0.99999), magnitude(0.00009), magnitude(f32::NAN)), (10000, -10000, -9999, 0, 0));
        // the hand-built data format: 8 axes, 4 hats, 128 buttons inside the 272-byte state
        let format = joystick_format();
        assert_eq!(format.len(), 140);
        assert_eq!(std::mem::size_of::<DIJOYSTATE2>(), 272);
        assert!(format.iter().all(|o| o.dwOfs < 176));
    }
}
