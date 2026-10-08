// SPDX-License-Identifier: GPL-3.0-or-later

//! The FMOD functions acs.exe imports, bound at run time to the player's own `fmod64.dll` and
//! `fmodstudio64.dll` (FMOD Studio 1.08.12, found in the Assetto Corsa folder). Nothing of FMOD
//! is linked or shipped: the functions are looked up by their exported names, the same names
//! acs.exe's import table carries.
//!
//! Every function here has the exact C ABI of the export, logs the call (see [`super::log`])
//! and forwards it. `tools/audio_oracle` puts these same functions into the import table of the
//! acs.exe image it maps, which is how the game's own sound code and the port come to write
//! call logs in one format.

use std::ffi::{c_char, c_void, CStr, CString, OsStr};
use std::fmt::Write as _;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::OnceLock;

use super::log::{hex_words, CallLog, LOG, LOGGING};
use super::types::*;

/// Returned by every function while the DLLs are not loaded (not an FMOD code).
pub const ERR_NOT_LOADED: i32 = -1;

/// Where the mix goes.
#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    /// The sound card, in real time: FMOD picks the device as acs.exe has it do.
    Device,
    /// A WAV file, written by FMOD's non-real-time writer: every `Studio::System::update`
    /// mixes exactly one block of `block` samples at `rate` Hz, on the calling thread.
    /// Nothing reaches the speakers.
    WavNrt { file: PathBuf, rate: i32, block: u32 },
    /// As `WavNrt` without the file.
    NoSoundNrt { rate: i32, block: u32 },
}

type Module = *mut c_void;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> Module;
    fn GetProcAddress(module: Module, name: *const u8) -> *mut c_void;
    fn GetLastError() -> u32;
}

const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x8;

unsafe fn symbol(module: Module, name: &str) -> Result<usize, String> {
    let c = CString::new(name).unwrap();
    let p = GetProcAddress(module, c.as_ptr().cast());
    if p.is_null() {
        Err(format!("the FMOD DLL has no export {name}"))
    } else {
        Ok(p as usize)
    }
}

/// Functions the layer itself needs and acs.exe does not import.
struct Extra {
    set_output: unsafe extern "C" fn(RawHandle, i32) -> i32,
    set_dsp_buffer_size: unsafe extern "C" fn(RawHandle, u32, i32) -> i32,
    flush_sample_loading: unsafe extern "C" fn(RawHandle) -> i32,
}

pub struct Api {
    pub fns: Fns,
    extra: Extra,
    output: Output,
    wav_file: CString,
}

// SAFETY: function addresses and immutable settings.
unsafe impl Sync for Api {}
unsafe impl Send for Api {}

static API: OnceLock<Api> = OnceLock::new();

/// The loaded API, if [`load`] has succeeded in this process.
pub fn api() -> Option<&'static Api> {
    API.get()
}

/// Loads the two FMOD DLLs from `folder` (the Assetto Corsa folder). Once per process: a second
/// call returns the first result's API whatever its arguments.
pub fn load(folder: &Path, output: Output) -> Result<&'static Api, String> {
    if let Some(api) = API.get() {
        return Ok(api);
    }
    let open = |name: &str| -> Result<Module, String> {
        let path = folder.join(name);
        if !path.is_file() {
            return Err(format!("{} not found", path.display()));
        }
        let wide: Vec<u16> = OsStr::new(&path).encode_wide().chain(Some(0)).collect();
        // SAFETY: a terminated path; loading runs FMOD's own DllMain, as it does in the game.
        let module = unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH) };
        if module.is_null() {
            return Err(format!("{} could not be loaded (error {})", path.display(), unsafe { GetLastError() }));
        }
        Ok(module)
    };
    // fmodstudio64.dll imports fmod64.dll: loaded first, it is found by name
    let low = open("fmod64.dll")?;
    let studio = open("fmodstudio64.dll")?;
    // SAFETY: the signatures are those of the exports' decorated names.
    let (fns, extra) = unsafe {
        (
            Fns::load(low, studio)?,
            Extra {
                set_output: std::mem::transmute::<usize, unsafe extern "C" fn(RawHandle, i32) -> i32>(symbol(
                    low,
                    "?setOutput@System@FMOD@@QEAA?AW4FMOD_RESULT@@W4FMOD_OUTPUTTYPE@@@Z",
                )?),
                set_dsp_buffer_size: std::mem::transmute::<usize, unsafe extern "C" fn(RawHandle, u32, i32) -> i32>(
                    symbol(low, "?setDSPBufferSize@System@FMOD@@QEAA?AW4FMOD_RESULT@@IH@Z")?,
                ),
                flush_sample_loading: std::mem::transmute::<usize, unsafe extern "C" fn(RawHandle) -> i32>(symbol(
                    studio,
                    "?flushSampleLoading@System@Studio@FMOD@@QEAA?AW4FMOD_RESULT@@XZ",
                )?),
            },
        )
    };
    let wav_file = match &output {
        Output::WavNrt { file, .. } => {
            CString::new(file.to_string_lossy().into_owned()).map_err(|_| "bad WAV file name".to_string())?
        }
        _ => CString::default(),
    };
    Ok(API.get_or_init(|| Api { fns, extra, output, wav_file }))
}

impl Api {
    pub fn output(&self) -> &Output {
        &self.output
    }

    /// Is the mix driven by `update` calls (no sound card, no clock)?
    pub fn non_real_time(&self) -> bool {
        !matches!(self.output, Output::Device)
    }
}

// --- arguments as the log prints them ----------------------------------------------------------

/// One argument of a logged call.
pub trait Arg {
    /// Printed before the arrow: what the caller passed.
    fn pre(&self, _log: &mut CallLog, _text: &mut String) {}
    /// Printed after the result, only when the call succeeded: what FMOD gave back.
    fn post(&self, _log: &mut CallLog, _text: &mut String) {}
    /// The address, if this is an FMOD object.
    fn address(&self) -> usize {
        0
    }
}

impl Arg for i32 {
    fn pre(&self, _: &mut CallLog, text: &mut String) {
        let _ = write!(text, " {self}");
    }
}
impl Arg for u32 {
    fn pre(&self, _: &mut CallLog, text: &mut String) {
        let _ = write!(text, " {self:#x}");
    }
}
impl Arg for f32 {
    fn pre(&self, _: &mut CallLog, text: &mut String) {
        let _ = write!(text, " {:08x}", self.to_bits());
    }
}
impl Arg for bool {
    fn pre(&self, _: &mut CallLog, text: &mut String) {
        let _ = write!(text, " {}", *self as u8);
    }
}

/// An FMOD object passed in.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Handle(pub RawHandle);
impl Arg for Handle {
    fn pre(&self, log: &mut CallLog, text: &mut String) {
        let id = log.id(self.0 as usize);
        let _ = write!(text, " {id}");
    }
    fn address(&self) -> usize {
        self.0 as usize
    }
}

/// A C string passed in.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Str(pub *const c_char);
impl Arg for Str {
    fn pre(&self, _: &mut CallLog, text: &mut String) {
        if self.0.is_null() {
            text.push_str(" null");
        } else {
            // SAFETY: the API takes a terminated string here.
            let s = unsafe { CStr::from_ptr(self.0) }.to_string_lossy();
            let _ = write!(text, " \"{s}\"");
        }
    }
}

/// Plain data passed in by address.
#[repr(transparent)]
pub struct In<T>(pub *const T);
impl<T> Clone for In<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for In<T> {}
impl<T: Pod> Arg for In<T> {
    fn pre(&self, _: &mut CallLog, text: &mut String) {
        text.push(' ');
        if self.0.is_null() {
            text.push_str("null");
        } else {
            // SAFETY: `T` is plain data and the API reads it at this address.
            hex_words(text, unsafe { std::slice::from_raw_parts(self.0.cast::<u8>(), size_of::<T>()) });
        }
    }
}

/// Plain data FMOD writes, part of the log.
#[repr(transparent)]
pub struct Out<T>(pub *mut T);
impl<T> Clone for Out<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Out<T> {}
impl<T: Pod> Arg for Out<T> {
    fn post(&self, _: &mut CallLog, text: &mut String) {
        text.push(' ');
        if self.0.is_null() {
            text.push_str("null");
        } else {
            // SAFETY: FMOD has just written a `T` here.
            hex_words(text, unsafe { std::slice::from_raw_parts(self.0.cast::<u8>(), size_of::<T>()) });
        }
    }
}

/// An FMOD object handed out.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct OutHandle(pub *mut RawHandle);
impl Arg for OutHandle {
    fn post(&self, log: &mut CallLog, text: &mut String) {
        if !self.0.is_null() {
            // SAFETY: FMOD has just written the object's address here.
            let id = log.id(unsafe { *self.0 } as usize);
            let _ = write!(text, " {id}");
        }
    }
}

/// Something the log leaves out: machine-dependent results (device names, CPU load), callbacks.
#[repr(transparent)]
pub struct Quiet<T>(pub *mut T);
impl<T> Clone for Quiet<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Quiet<T> {}
impl<T> Arg for Quiet<T> {}

/// A settings structure that starts with its own size (`cbsize`): logged whole.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Sized(pub *mut c_void);
impl Arg for Sized {
    fn pre(&self, _: &mut CallLog, text: &mut String) {
        text.push(' ');
        if self.0.is_null() {
            text.push_str("null");
            return;
        }
        // SAFETY: the structure's first member is its size in bytes.
        unsafe {
            let size = (*self.0.cast::<i32>()).clamp(0, 4096) as usize;
            hex_words(text, std::slice::from_raw_parts(self.0.cast::<u8>(), size));
        }
    }
}

fn log_call(name: &str, args: &[&dyn Arg], result: i32, forget: bool) {
    let mut guard = LOG.lock().unwrap();
    let Some(log) = guard.as_mut() else { return };
    let mut text = String::with_capacity(96);
    text.push_str(name);
    for arg in args {
        arg.pre(log, &mut text);
    }
    let _ = write!(text, " -> {result}");
    if result == FMOD_OK {
        for arg in args {
            arg.post(log, &mut text);
        }
    }
    if forget {
        if let Some(first) = args.first() {
            log.forget(first.address());
        }
    }
    log.push(&text);
}

macro_rules! pick {
    (low, $low:expr, $studio:expr) => {
        $low
    };
    (studio, $low:expr, $studio:expr) => {
        $studio
    };
}

macro_rules! flag {
    () => {
        false
    };
    (forget) => {
        true
    };
}

macro_rules! fmod_api {
    (
        auto { $( $dll:ident $sym:literal fn $name:ident ( $($arg:ident : $ty:ty),* ) $([$flag:ident])? ; )* }
        manual { $( $mdll:ident $msym:literal fn $mname:ident ( $($marg:ident : $mty:ty),* ) ; )* }
    ) => {
        /// The exports, as loaded.
        pub struct Fns {
            $( pub $name: unsafe extern "C" fn($($ty),*) -> i32, )*
            $( pub $mname: unsafe extern "C" fn($($mty),*) -> i32, )*
        }

        impl Fns {
            unsafe fn load(low: Module, studio: Module) -> Result<Fns, String> {
                Ok(Fns {
                    $( $name: std::mem::transmute::<usize, unsafe extern "C" fn($($ty),*) -> i32>(
                        symbol(pick!($dll, low, studio), $sym)?), )*
                    $( $mname: std::mem::transmute::<usize, unsafe extern "C" fn($($mty),*) -> i32>(
                        symbol(pick!($mdll, low, studio), $msym)?), )*
                })
            }
        }

        $(
            /// Logged, then forwarded to the export of the same name.
            ///
            /// # Safety
            /// As the FMOD function: valid objects and pointers.
            pub unsafe extern "C" fn $name($($arg: $ty),*) -> i32 {
                let Some(api) = api() else { return ERR_NOT_LOADED };
                let result = (api.fns.$name)($($arg),*);
                if LOGGING.load(Ordering::Relaxed) {
                    log_call(stringify!($name), &[$(&$arg as &dyn Arg),*], result, flag!($($flag)?));
                }
                result
            }
        )*

        /// Every FMOD import of acs.exe with the function of this layer that stands in for it.
        pub fn imports() -> Vec<(&'static str, usize)> {
            vec![
                $( ($sym, $name as *const () as usize), )*
                $( ($msym, $mname as *const () as usize), )*
            ]
        }
    };
}

fmod_api! {
    auto {
        // --- fmod64.dll: FMOD::System ---
        low "?setDriver@System@FMOD@@QEAA?AW4FMOD_RESULT@@H@Z"
            fn system_set_driver(this: Handle, driver: i32);
        low "?getDriverInfo@System@FMOD@@QEAA?AW4FMOD_RESULT@@HPEADHPEAUFMOD_GUID@@PEAHPEAW4FMOD_SPEAKERMODE@@2@Z"
            fn system_get_driver_info(this: Handle, id: i32, name: Quiet<c_char>, name_len: i32, guid: Quiet<Guid>, rate: Quiet<i32>, mode: Quiet<i32>, channels: Quiet<i32>);
        low "?setSoftwareChannels@System@FMOD@@QEAA?AW4FMOD_RESULT@@H@Z"
            fn system_set_software_channels(this: Handle, count: i32);
        low "?getOutput@System@FMOD@@QEAA?AW4FMOD_RESULT@@PEAW4FMOD_OUTPUTTYPE@@@Z"
            fn system_get_output(this: Handle, output: Quiet<i32>);
        low "FMOD_System_GetDriver"
            fn c_system_get_driver(this: Handle, driver: Quiet<i32>);
        low "?setSoftwareFormat@System@FMOD@@QEAA?AW4FMOD_RESULT@@HW4FMOD_SPEAKERMODE@@H@Z"
            fn system_set_software_format(this: Handle, rate: i32, mode: i32, raw_speakers: i32);
        low "?getSoftwareFormat@System@FMOD@@QEAA?AW4FMOD_RESULT@@PEAHPEAW4FMOD_SPEAKERMODE@@0@Z"
            fn system_get_software_format(this: Handle, rate: Quiet<i32>, mode: Quiet<i32>, raw_speakers: Quiet<i32>);
        low "?getDriver@System@FMOD@@QEAA?AW4FMOD_RESULT@@PEAH@Z"
            fn system_get_driver(this: Handle, driver: Quiet<i32>);
        low "?getVersion@System@FMOD@@QEAA?AW4FMOD_RESULT@@PEAI@Z"
            fn system_get_version(this: Handle, version: Out<u32>);
        low "?getChannelsPlaying@System@FMOD@@QEAA?AW4FMOD_RESULT@@PEAH0@Z"
            fn system_get_channels_playing(this: Handle, channels: Quiet<i32>, real: Quiet<i32>);
        low "?getDSPBufferSize@System@FMOD@@QEAA?AW4FMOD_RESULT@@PEAIPEAH@Z"
            fn system_get_dsp_buffer_size(this: Handle, length: Quiet<u32>, count: Quiet<i32>);
        low "?setAdvancedSettings@System@FMOD@@QEAA?AW4FMOD_RESULT@@PEAUFMOD_ADVANCEDSETTINGS@@@Z"
            fn system_set_advanced_settings(this: Handle, settings: Sized);
        low "?setCallback@System@FMOD@@QEAA?AW4FMOD_RESULT@@P6A?AW43@PEAUFMOD_SYSTEM@@IPEAX11@ZI@Z"
            fn system_set_callback(this: Handle, callback: Quiet<c_void>, mask: u32);
        low "?get3DSettings@System@FMOD@@QEAA?AW4FMOD_RESULT@@PEAM00@Z"
            fn system_get_3d_settings(this: Handle, doppler: Out<f32>, distance: Out<f32>, rolloff: Out<f32>);
        low "?getNumDrivers@System@FMOD@@QEAA?AW4FMOD_RESULT@@PEAH@Z"
            fn system_get_num_drivers(this: Handle, count: Quiet<i32>);
        low "?set3DSettings@System@FMOD@@QEAA?AW4FMOD_RESULT@@MMM@Z"
            fn system_set_3d_settings(this: Handle, doppler: f32, distance: f32, rolloff: f32);
        low "?createReverb3D@System@FMOD@@QEAA?AW4FMOD_RESULT@@PEAPEAVReverb3D@2@@Z"
            fn system_create_reverb_3d(this: Handle, reverb: OutHandle);
        low "?getMasterChannelGroup@System@FMOD@@QEAA?AW4FMOD_RESULT@@PEAPEAVChannelGroup@2@@Z"
            fn system_get_master_channel_group(this: Handle, group: OutHandle);
        low "?createGeometry@System@FMOD@@QEAA?AW4FMOD_RESULT@@HHPEAPEAVGeometry@2@@Z"
            fn system_create_geometry(this: Handle, max_polygons: i32, max_vertices: i32, geometry: OutHandle);
        low "?getGeometryOcclusion@System@FMOD@@QEAA?AW4FMOD_RESULT@@PEBUFMOD_VECTOR@@0PEAM1@Z"
            fn system_get_geometry_occlusion(this: Handle, listener: In<Vector>, source: In<Vector>, direct: Out<f32>, reverb: Out<f32>);
        low "?setGeometrySettings@System@FMOD@@QEAA?AW4FMOD_RESULT@@M@Z"
            fn system_set_geometry_settings(this: Handle, max_world_size: f32);
        // --- fmod64.dll: Reverb3D, Geometry, ChannelControl ---
        low "?setActive@Reverb3D@FMOD@@QEAA?AW4FMOD_RESULT@@_N@Z"
            fn reverb_set_active(this: Handle, active: bool);
        low "?setProperties@Reverb3D@FMOD@@QEAA?AW4FMOD_RESULT@@PEBUFMOD_REVERB_PROPERTIES@@@Z"
            fn reverb_set_properties(this: Handle, properties: In<ReverbProperties>);
        low "?get3DAttributes@Reverb3D@FMOD@@QEAA?AW4FMOD_RESULT@@PEAUFMOD_VECTOR@@PEAM1@Z"
            fn reverb_get_3d_attributes(this: Handle, position: Out<Vector>, min_distance: Out<f32>, max_distance: Out<f32>);
        low "?set3DAttributes@Reverb3D@FMOD@@QEAA?AW4FMOD_RESULT@@PEBUFMOD_VECTOR@@MM@Z"
            fn reverb_set_3d_attributes(this: Handle, position: In<Vector>, min_distance: f32, max_distance: f32);
        low "?release@Reverb3D@FMOD@@QEAA?AW4FMOD_RESULT@@XZ"
            fn reverb_release(this: Handle) [forget];
        low "?setActive@Geometry@FMOD@@QEAA?AW4FMOD_RESULT@@_N@Z"
            fn geometry_set_active(this: Handle, active: bool);
        low "?setPolygonAttributes@Geometry@FMOD@@QEAA?AW4FMOD_RESULT@@HMM_N@Z"
            fn geometry_set_polygon_attributes(this: Handle, index: i32, direct: f32, reverb: f32, double_sided: bool);
        low "?getNumPolygons@Geometry@FMOD@@QEAA?AW4FMOD_RESULT@@PEAH@Z"
            fn geometry_get_num_polygons(this: Handle, count: Out<i32>);
        low "?release@Geometry@FMOD@@QEAA?AW4FMOD_RESULT@@XZ"
            fn geometry_release(this: Handle) [forget];
        low "?setReverbProperties@ChannelControl@FMOD@@QEAA?AW4FMOD_RESULT@@HM@Z"
            fn channel_set_reverb_properties(this: Handle, instance: i32, wet: f32);
        low "?setVolume@ChannelControl@FMOD@@QEAA?AW4FMOD_RESULT@@M@Z"
            fn channel_set_volume(this: Handle, volume: f32);
        // --- fmodstudio64.dll ---
        studio "?create@System@Studio@FMOD@@SA?AW4FMOD_RESULT@@PEAPEAV123@I@Z"
            fn studio_create(system: OutHandle, header_version: u32);
        studio "?getCPUUsage@System@Studio@FMOD@@QEBA?AW4FMOD_RESULT@@PEAUFMOD_STUDIO_CPU_USAGE@@@Z"
            fn studio_get_cpu_usage(this: Handle, usage: Quiet<c_void>);
        studio "?getBufferUsage@System@Studio@FMOD@@QEBA?AW4FMOD_RESULT@@PEAUFMOD_STUDIO_BUFFER_USAGE@@@Z"
            fn studio_get_buffer_usage(this: Handle, usage: Quiet<c_void>);
        studio "?unload@Bank@Studio@FMOD@@QEAA?AW4FMOD_RESULT@@XZ"
            fn bank_unload(this: Handle) [forget];
        studio "?setParameterValue@EventInstance@Studio@FMOD@@QEAA?AW4FMOD_RESULT@@PEBDM@Z"
            fn event_set_parameter_value(this: Handle, name: Str, value: f32);
        studio "?release@EventInstance@Studio@FMOD@@QEAA?AW4FMOD_RESULT@@XZ"
            fn event_release(this: Handle) [forget];
        studio "?getPlaybackState@EventInstance@Studio@FMOD@@QEBA?AW4FMOD_RESULT@@PEAW4FMOD_STUDIO_PLAYBACK_STATE@@@Z"
            fn event_get_playback_state(this: Handle, state: Out<i32>);
        studio "?setTimelinePosition@EventInstance@Studio@FMOD@@QEAA?AW4FMOD_RESULT@@H@Z"
            fn event_set_timeline_position(this: Handle, position: i32);
        studio "?stop@EventInstance@Studio@FMOD@@QEAA?AW4FMOD_RESULT@@W4FMOD_STUDIO_STOP_MODE@@@Z"
            fn event_stop(this: Handle, mode: i32);
        studio "?start@EventInstance@Studio@FMOD@@QEAA?AW4FMOD_RESULT@@XZ"
            fn event_start(this: Handle);
        studio "?setPaused@EventInstance@Studio@FMOD@@QEAA?AW4FMOD_RESULT@@_N@Z"
            fn event_set_paused(this: Handle, paused: bool);
        studio "?getPaused@EventInstance@Studio@FMOD@@QEBA?AW4FMOD_RESULT@@PEA_N@Z"
            fn event_get_paused(this: Handle, paused: Out<bool>);
        studio "?set3DAttributes@EventInstance@Studio@FMOD@@QEAA?AW4FMOD_RESULT@@PEBUFMOD_3D_ATTRIBUTES@@@Z"
            fn event_set_3d_attributes(this: Handle, attributes: In<Attributes3d>);
        studio "?get3DAttributes@EventInstance@Studio@FMOD@@QEBA?AW4FMOD_RESULT@@PEAUFMOD_3D_ATTRIBUTES@@@Z"
            fn event_get_3d_attributes(this: Handle, attributes: Out<Attributes3d>);
        studio "?setPitch@EventInstance@Studio@FMOD@@QEAA?AW4FMOD_RESULT@@M@Z"
            fn event_set_pitch(this: Handle, pitch: f32);
        studio "?setVolume@EventInstance@Studio@FMOD@@QEAA?AW4FMOD_RESULT@@M@Z"
            fn event_set_volume(this: Handle, volume: f32);
        studio "?unloadSampleData@EventDescription@Studio@FMOD@@QEAA?AW4FMOD_RESULT@@XZ"
            fn description_unload_sample_data(this: Handle);
        studio "?loadSampleData@EventDescription@Studio@FMOD@@QEAA?AW4FMOD_RESULT@@XZ"
            fn description_load_sample_data(this: Handle);
        studio "?createInstance@EventDescription@Studio@FMOD@@QEBA?AW4FMOD_RESULT@@PEAPEAVEventInstance@23@@Z"
            fn description_create_instance(this: Handle, instance: OutHandle);
        studio "?getMaximumDistance@EventDescription@Studio@FMOD@@QEBA?AW4FMOD_RESULT@@PEAM@Z"
            fn description_get_maximum_distance(this: Handle, distance: Out<f32>);
        studio "?setAdvancedSettings@System@Studio@FMOD@@QEAA?AW4FMOD_RESULT@@PEAUFMOD_STUDIO_ADVANCEDSETTINGS@@@Z"
            fn studio_set_advanced_settings(this: Handle, settings: Sized);
        studio "?release@System@Studio@FMOD@@QEAA?AW4FMOD_RESULT@@XZ"
            fn studio_release(this: Handle) [forget];
        studio "?flushCommands@System@Studio@FMOD@@QEAA?AW4FMOD_RESULT@@XZ"
            fn studio_flush_commands(this: Handle);
        studio "?getLowLevelSystem@System@Studio@FMOD@@QEBA?AW4FMOD_RESULT@@PEAPEAV13@@Z"
            fn studio_get_low_level_system(this: Handle, system: OutHandle);
        studio "?getEventByID@System@Studio@FMOD@@QEBA?AW4FMOD_RESULT@@PEBUFMOD_GUID@@PEAPEAVEventDescription@23@@Z"
            fn studio_get_event_by_id(this: Handle, guid: In<Guid>, description: OutHandle);
        studio "?getListenerAttributes@System@Studio@FMOD@@QEBA?AW4FMOD_RESULT@@HPEAUFMOD_3D_ATTRIBUTES@@@Z"
            fn studio_get_listener_attributes(this: Handle, listener: i32, attributes: Out<Attributes3d>);
        studio "?setListenerAttributes@System@Studio@FMOD@@QEAA?AW4FMOD_RESULT@@HPEBUFMOD_3D_ATTRIBUTES@@@Z"
            fn studio_set_listener_attributes(this: Handle, listener: i32, attributes: In<Attributes3d>);
        studio "?loadBankFile@System@Studio@FMOD@@QEAA?AW4FMOD_RESULT@@PEBDIPEAPEAVBank@23@@Z"
            fn studio_load_bank_file(this: Handle, file: Str, flags: u32, bank: OutHandle);
    }
    manual {
        low "?addPolygon@Geometry@FMOD@@QEAA?AW4FMOD_RESULT@@MM_NHPEBUFMOD_VECTOR@@PEAH@Z"
            fn geometry_add_polygon(this: Handle, direct: f32, reverb: f32, double_sided: bool, count: i32, vertices: *const Vector, index: *mut i32);
        studio "?getChannelGroup@EventInstance@Studio@FMOD@@QEBA?AW4FMOD_RESULT@@PEAPEAVChannelGroup@3@@Z"
            fn event_get_channel_group(this: Handle, group: *mut RawHandle);
        studio "?initialize@System@Studio@FMOD@@QEAA?AW4FMOD_RESULT@@HIIPEAX@Z"
            fn studio_initialize(this: Handle, max_channels: i32, studio_flags: u32, flags: u32, extra: *mut c_void);
        studio "?registerPlugin@System@Studio@FMOD@@QEAA?AW4FMOD_RESULT@@PEBUFMOD_DSP_DESCRIPTION@@@Z"
            fn studio_register_plugin(this: Handle, description: *const c_void);
        studio "?update@System@Studio@FMOD@@QEAA?AW4FMOD_RESULT@@XZ"
            fn studio_update(this: Handle);
    }
}

/// `Geometry::addPolygon`: the vertices are logged too.
///
/// # Safety
/// As the FMOD function.
pub unsafe extern "C" fn geometry_add_polygon(
    this: Handle,
    direct: f32,
    reverb: f32,
    double_sided: bool,
    count: i32,
    vertices: *const Vector,
    index: *mut i32,
) -> i32 {
    let Some(api) = api() else { return ERR_NOT_LOADED };
    let result = (api.fns.geometry_add_polygon)(this, direct, reverb, double_sided, count, vertices, index);
    if LOGGING.load(Ordering::Relaxed) {
        let mut guard = LOG.lock().unwrap();
        if let Some(log) = guard.as_mut() {
            let mut text = String::from("geometry_add_polygon");
            this.pre(log, &mut text);
            direct.pre(log, &mut text);
            reverb.pre(log, &mut text);
            double_sided.pre(log, &mut text);
            count.pre(log, &mut text);
            text.push(' ');
            if !vertices.is_null() && count > 0 {
                hex_words(&mut text, std::slice::from_raw_parts(vertices.cast::<u8>(), count as usize * 12));
            }
            let _ = write!(text, " -> {result}");
            if result == FMOD_OK && !index.is_null() {
                let _ = write!(text, " {}", *index);
            }
            log.push(&text);
        }
    }
    result
}

/// `EventInstance::getChannelGroup`. A channel group lives and dies inside FMOD with its event,
/// so every group handed out counts as a new object in the log.
///
/// # Safety
/// As the FMOD function.
pub unsafe extern "C" fn event_get_channel_group(this: Handle, group: *mut RawHandle) -> i32 {
    let Some(api) = api() else { return ERR_NOT_LOADED };
    let result = (api.fns.event_get_channel_group)(this, group);
    if LOGGING.load(Ordering::Relaxed) {
        let mut guard = LOG.lock().unwrap();
        if let Some(log) = guard.as_mut() {
            let mut text = String::from("event_get_channel_group");
            this.pre(log, &mut text);
            let _ = write!(text, " -> {result}");
            if result == FMOD_OK && !group.is_null() {
                let id = log.fresh_id(*group as usize);
                let _ = write!(text, " {id}");
            }
            log.push(&text);
        }
    }
    result
}

/// `Studio::System::initialize`. The caller's arguments are logged as given; when the layer was
/// loaded for a non-real-time output, the output type, block size and format are set first and
/// the Studio system is made synchronous, so that the mix depends on the calls alone.
///
/// # Safety
/// As the FMOD function.
pub unsafe extern "C" fn studio_initialize(
    this: Handle,
    max_channels: i32,
    studio_flags: u32,
    flags: u32,
    extra: *mut c_void,
) -> i32 {
    let Some(api) = api() else { return ERR_NOT_LOADED };
    let mut studio_flags_used = studio_flags;
    let mut extra_used = extra;
    let nrt = match &api.output {
        Output::Device => None,
        Output::WavNrt { rate, block, .. } => Some((OUTPUTTYPE_WAVWRITER_NRT, *rate, *block)),
        Output::NoSoundNrt { rate, block } => Some((OUTPUTTYPE_NOSOUND_NRT, *rate, *block)),
    };
    if let Some((kind, rate, block)) = nrt {
        let mut low: RawHandle = std::ptr::null_mut();
        let found = (api.fns.studio_get_low_level_system)(this, OutHandle(&mut low));
        if found != FMOD_OK || low.is_null() {
            return found;
        }
        (api.extra.set_output)(low, kind);
        (api.extra.set_dsp_buffer_size)(low, block, 4);
        (api.fns.system_set_software_format)(Handle(low), rate, SPEAKERMODE_STEREO, 0);
        studio_flags_used |= STUDIO_INIT_SYNCHRONOUS_UPDATE;
        if kind == OUTPUTTYPE_WAVWRITER_NRT {
            extra_used = api.wav_file.as_ptr() as *mut c_void;
        }
    }
    let result = (api.fns.studio_initialize)(this, max_channels, studio_flags_used, flags, extra_used);
    if LOGGING.load(Ordering::Relaxed) {
        log_call("studio_initialize", &[&this, &max_channels, &studio_flags, &flags], result, false);
    }
    result
}

/// `Studio::System::registerPlugin`: the description's plain members are logged (its callbacks
/// are addresses of the caller).
///
/// # Safety
/// As the FMOD function.
pub unsafe extern "C" fn studio_register_plugin(this: Handle, description: *const c_void) -> i32 {
    let Some(api) = api() else { return ERR_NOT_LOADED };
    let result = (api.fns.studio_register_plugin)(this, description);
    if LOGGING.load(Ordering::Relaxed) {
        let mut guard = LOG.lock().unwrap();
        if let Some(log) = guard.as_mut() {
            let mut text = String::from("studio_register_plugin");
            this.pre(log, &mut text);
            if !description.is_null() {
                super::dsp_abi::describe(description, &mut text);
            }
            let _ = write!(text, " -> {result}");
            log.push(&text);
        }
    }
    result
}

/// `Studio::System::update`. For a non-real-time output the sample loading the update has set
/// off is finished before the call returns: what plays in the next block then never depends on
/// how fast the disk was.
///
/// # Safety
/// As the FMOD function.
pub unsafe extern "C" fn studio_update(this: Handle) -> i32 {
    let Some(api) = api() else { return ERR_NOT_LOADED };
    let result = (api.fns.studio_update)(this);
    if api.non_real_time() {
        (api.extra.flush_sample_loading)(this.0);
    }
    if LOGGING.load(Ordering::Relaxed) {
        log_call("studio_update", &[&this], result, false);
    }
    result
}
