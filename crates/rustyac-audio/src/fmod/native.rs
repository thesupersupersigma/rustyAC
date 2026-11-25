// SPDX-License-Identifier: GPL-3.0-or-later

//! FMOD Engine 2.03 off Windows: Firelight's `libfmod` / `libfmodstudio` (the Linux zips carry
//! them next to the program), loaded at run time and called through their C interface behind
//! the 1.08 functions of [`super::raw`]. Most of acs.exe's imports have a 2.03 C function with
//! the same arguments; the others are translated here as `web/sound.js` translates them for the
//! browser (the same table, the same answers), so the call log stays the desktop's:
//!
//!   getVersion            -> 0x10812, the version the game's code checks for
//!   getLowLevelSystem     -> getCoreSystem
//!   setParameterValue     -> setParameterByName
//!   getMaximumDistance    -> getMinMaxDistance (the max)
//!   get/setListenerAttributes -> no attenuation position; 1.08's default before the first set
//!   Studio::System::create    -> 2.03's header version (2.03 refuses 1.08's)
//!   the advanced settings -> 2.03's structures, with the members the game sets
//!   getCPUUsage           -> 2.03's two structures as 1.08's one
//!   bool arguments        -> FMOD_BOOL (an int in the C interface)
//!
//! Unlike the browser's build, the native one has `Geometry::addPolygon` and runs the game's
//! two DSP plug-ins from the banks (their 1.08 ABI is 2.03's).

use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;

use libloading::Library;

use super::raw::{Extra, Fns, Handle, Out, OutHandle, Quiet, Sized, Str};
use super::types::*;

const CORE: &str = "libfmod.so.14";
const STUDIO: &str = "libfmodstudio.so.14";
/// FMOD_VERSION of the 2.03.15 headers this table was written from.
const HEADER_VERSION: u32 = 0x0002_0315;

/// `FMOD_ADVANCEDSETTINGS` (2.03)
#[repr(C)]
struct Advanced {
    cb_size: i32,
    max_codecs: [i32; 7],
    asio_num_channels: i32,
    asio_channel_list: *mut c_void,
    asio_speaker_list: *mut c_void,
    vol0virtualvol: f32,
    default_decode_buffer_size: u32,
    profile_port: u16,
    geometry_max_fade_time: u32,
    distance_filter_center_freq: f32,
    reverb_3d_instance: i32,
    dsp_buffer_pool_size: i32,
    resampler_method: i32,
    random_seed: u32,
    max_convolution_threads: i32,
    max_spatial_objects: i32,
}

/// `FMOD_STUDIO_ADVANCEDSETTINGS` (2.03)
#[repr(C)]
struct StudioAdvanced {
    cbsize: i32,
    commandqueuesize: u32,
    handleinitialsize: u32,
    studioupdateperiod: i32,
    idlesampledatapoolsize: i32,
    streamingscheduledelay: u32,
    encryptionkey: *const c_char,
}

/// The 2.03 functions the translations below call.
struct Native {
    studio_create: unsafe extern "C" fn(*mut RawHandle, u32) -> i32,
    cpu_usage: unsafe extern "C" fn(RawHandle, *mut f32, *mut [f32; 6]) -> i32,
    set_parameter_by_name: unsafe extern "C" fn(RawHandle, *const c_char, f32, i32) -> i32,
    set_paused: unsafe extern "C" fn(RawHandle, i32) -> i32,
    get_paused: unsafe extern "C" fn(RawHandle, *mut i32) -> i32,
    reverb_set_active: unsafe extern "C" fn(RawHandle, i32) -> i32,
    geometry_set_active: unsafe extern "C" fn(RawHandle, i32) -> i32,
    geometry_set_polygon_attributes: unsafe extern "C" fn(RawHandle, i32, f32, f32, i32) -> i32,
    geometry_add_polygon: unsafe extern "C" fn(RawHandle, f32, f32, i32, i32, *const Vector, *mut i32) -> i32,
    min_max_distance: unsafe extern "C" fn(RawHandle, *mut f32, *mut f32) -> i32,
    studio_get_advanced: unsafe extern "C" fn(RawHandle, *mut StudioAdvanced) -> i32,
    studio_set_advanced: unsafe extern "C" fn(RawHandle, *mut StudioAdvanced) -> i32,
    core_get_advanced: unsafe extern "C" fn(RawHandle, *mut Advanced) -> i32,
    core_set_advanced: unsafe extern "C" fn(RawHandle, *mut Advanced) -> i32,
    get_listener: unsafe extern "C" fn(RawHandle, i32, *mut Attributes3d, *mut Vector) -> i32,
    set_listener: unsafe extern "C" fn(RawHandle, i32, *const Attributes3d, *const Vector) -> i32,
}

static LIBS: OnceLock<(Library, Library)> = OnceLock::new();
static NATIVE: OnceLock<Native> = OnceLock::new();
/// The listeners set since the Studio system was made (one bit each).
static LISTENERS: AtomicU32 = AtomicU32::new(0);

fn n() -> &'static Native {
    NATIVE.get().expect("FMOD is loaded")
}

/// One library: next to the program first (the release zips), then wherever the system's
/// loader finds it (`LD_LIBRARY_PATH`).
fn open(name: &str) -> Result<Library, String> {
    let beside = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|dir| dir.join(name))).filter(|p| p.is_file());
    // SAFETY: Firelight's library; loading it runs only its own initialisers.
    unsafe {
        match beside {
            Some(path) => Library::new(path),
            None => Library::new(name),
        }
    }
    .map_err(|e| format!("FMOD Engine 2.03 ({name}) was not found: {e}"))
}

/// A function of the library, as the type the field it goes into has.
unsafe fn get<F: Copy>(lib: &'static Library, name: &str) -> Result<F, String> {
    lib.get::<F>(name.as_bytes()).map(|s| *s).map_err(|e| format!("FMOD has no {name}: {e}"))
}

/// Loads both libraries (once per process) and binds every function of [`Fns`] and [`Extra`].
pub(super) fn load() -> Result<(Fns, Extra), String> {
    let (core, studio) = match LIBS.get() {
        Some(libs) => libs,
        None => {
            // the core first: the Studio library needs it by name
            let core = open(CORE)?;
            let studio = open(STUDIO)?;
            LIBS.get_or_init(|| (core, studio))
        }
    };
    // SAFETY: each name is the 2.03 C function whose arguments match the field's type.
    unsafe {
        let native = Native {
            studio_create: get(studio, "FMOD_Studio_System_Create")?,
            cpu_usage: get(studio, "FMOD_Studio_System_GetCPUUsage")?,
            set_parameter_by_name: get(studio, "FMOD_Studio_EventInstance_SetParameterByName")?,
            set_paused: get(studio, "FMOD_Studio_EventInstance_SetPaused")?,
            get_paused: get(studio, "FMOD_Studio_EventInstance_GetPaused")?,
            reverb_set_active: get(core, "FMOD_Reverb3D_SetActive")?,
            geometry_set_active: get(core, "FMOD_Geometry_SetActive")?,
            geometry_set_polygon_attributes: get(core, "FMOD_Geometry_SetPolygonAttributes")?,
            geometry_add_polygon: get(core, "FMOD_Geometry_AddPolygon")?,
            min_max_distance: get(studio, "FMOD_Studio_EventDescription_GetMinMaxDistance")?,
            studio_get_advanced: get(studio, "FMOD_Studio_System_GetAdvancedSettings")?,
            studio_set_advanced: get(studio, "FMOD_Studio_System_SetAdvancedSettings")?,
            core_get_advanced: get(core, "FMOD_System_GetAdvancedSettings")?,
            core_set_advanced: get(core, "FMOD_System_SetAdvancedSettings")?,
            get_listener: get(studio, "FMOD_Studio_System_GetListenerAttributes")?,
            set_listener: get(studio, "FMOD_Studio_System_SetListenerAttributes")?,
        };
        let _ = NATIVE.set(native);
        let fns = Fns {
            system_set_driver: get(core, "FMOD_System_SetDriver")?,
            system_get_driver_info: get(core, "FMOD_System_GetDriverInfo")?,
            system_set_software_channels: get(core, "FMOD_System_SetSoftwareChannels")?,
            system_get_output: get(core, "FMOD_System_GetOutput")?,
            c_system_get_driver: get(core, "FMOD_System_GetDriver")?,
            system_set_software_format: get(core, "FMOD_System_SetSoftwareFormat")?,
            system_get_software_format: get(core, "FMOD_System_GetSoftwareFormat")?,
            system_get_driver: get(core, "FMOD_System_GetDriver")?,
            system_get_version,
            system_get_channels_playing: get(core, "FMOD_System_GetChannelsPlaying")?,
            system_get_dsp_buffer_size: get(core, "FMOD_System_GetDSPBufferSize")?,
            // (the one callback the game sets, the device lost, has the same bit and arguments)
            system_set_callback: get(core, "FMOD_System_SetCallback")?,
            system_get_3d_settings: get(core, "FMOD_System_Get3DSettings")?,
            system_get_num_drivers: get(core, "FMOD_System_GetNumDrivers")?,
            system_set_3d_settings: get(core, "FMOD_System_Set3DSettings")?,
            system_create_reverb_3d: get(core, "FMOD_System_CreateReverb3D")?,
            system_get_master_channel_group: get(core, "FMOD_System_GetMasterChannelGroup")?,
            system_create_geometry: get(core, "FMOD_System_CreateGeometry")?,
            system_get_geometry_occlusion: get(core, "FMOD_System_GetGeometryOcclusion")?,
            system_set_geometry_settings: get(core, "FMOD_System_SetGeometrySettings")?,
            reverb_set_active,
            reverb_set_properties: get(core, "FMOD_Reverb3D_SetProperties")?,
            reverb_get_3d_attributes: get(core, "FMOD_Reverb3D_Get3DAttributes")?,
            reverb_set_3d_attributes: get(core, "FMOD_Reverb3D_Set3DAttributes")?,
            reverb_release: get(core, "FMOD_Reverb3D_Release")?,
            geometry_set_active,
            geometry_set_polygon_attributes,
            geometry_get_num_polygons: get(core, "FMOD_Geometry_GetNumPolygons")?,
            geometry_release: get(core, "FMOD_Geometry_Release")?,
            // (the game's channel controls are channel groups: the master's, its events')
            channel_set_reverb_properties: get(core, "FMOD_ChannelGroup_SetReverbProperties")?,
            channel_set_volume: get(core, "FMOD_ChannelGroup_SetVolume")?,
            studio_get_cpu_usage,
            studio_get_buffer_usage: get(studio, "FMOD_Studio_System_GetBufferUsage")?,
            event_set_parameter_value,
            event_release: get(studio, "FMOD_Studio_EventInstance_Release")?,
            event_get_playback_state: get(studio, "FMOD_Studio_EventInstance_GetPlaybackState")?,
            event_set_timeline_position: get(studio, "FMOD_Studio_EventInstance_SetTimelinePosition")?,
            event_stop: get(studio, "FMOD_Studio_EventInstance_Stop")?,
            event_start: get(studio, "FMOD_Studio_EventInstance_Start")?,
            event_set_paused,
            event_get_paused,
            event_set_3d_attributes: get(studio, "FMOD_Studio_EventInstance_Set3DAttributes")?,
            event_get_3d_attributes: get(studio, "FMOD_Studio_EventInstance_Get3DAttributes")?,
            event_set_pitch: get(studio, "FMOD_Studio_EventInstance_SetPitch")?,
            event_set_volume: get(studio, "FMOD_Studio_EventInstance_SetVolume")?,
            description_load_sample_data: get(studio, "FMOD_Studio_EventDescription_LoadSampleData")?,
            description_create_instance: get(studio, "FMOD_Studio_EventDescription_CreateInstance")?,
            description_get_maximum_distance,
            studio_set_advanced_settings,
            studio_get_low_level_system: get(studio, "FMOD_Studio_System_GetCoreSystem")?,
            studio_get_listener_attributes,
            studio_set_listener_attributes,
            geometry_add_polygon,
            event_get_channel_group: get(studio, "FMOD_Studio_EventInstance_GetChannelGroup")?,
            studio_initialize: get(studio, "FMOD_Studio_System_Initialize")?,
            studio_register_plugin: get(studio, "FMOD_Studio_System_RegisterPlugin")?,
            studio_update: get(studio, "FMOD_Studio_System_Update")?,
            studio_create,
            bank_unload: get(studio, "FMOD_Studio_Bank_Unload")?,
            description_unload_sample_data: get(studio, "FMOD_Studio_EventDescription_UnloadSampleData")?,
            studio_flush_commands: get(studio, "FMOD_Studio_System_FlushCommands")?,
            studio_get_event_by_id: get(studio, "FMOD_Studio_System_GetEventByID")?,
            studio_release: get(studio, "FMOD_Studio_System_Release")?,
            system_set_advanced_settings,
            studio_load_bank_file: get(studio, "FMOD_Studio_System_LoadBankFile")?,
        };
        let extra = Extra {
            set_output: get(core, "FMOD_System_SetOutput")?,
            set_dsp_buffer_size: get(core, "FMOD_System_SetDSPBufferSize")?,
            flush_sample_loading: get(studio, "FMOD_Studio_System_FlushSampleLoading")?,
            get_sample_loading_state: get(studio, "FMOD_Studio_EventDescription_GetSampleLoadingState")?,
        };
        Ok((fns, extra))
    }
}

// --- the translations ---------------------------------------------------------------------------

unsafe extern "C" fn system_get_version(_this: Handle, version: Out<u32>) -> i32 {
    if !version.0.is_null() {
        *version.0 = FMOD_VERSION;
    }
    FMOD_OK
}

unsafe extern "C" fn studio_create(system: OutHandle, _header_version: u32) -> i32 {
    LISTENERS.store(0, Ordering::Relaxed);
    (n().studio_create)(system.0, HEADER_VERSION)
}

unsafe extern "C" fn reverb_set_active(this: Handle, active: bool) -> i32 {
    (n().reverb_set_active)(this.0, active as i32)
}

unsafe extern "C" fn geometry_set_active(this: Handle, active: bool) -> i32 {
    (n().geometry_set_active)(this.0, active as i32)
}

unsafe extern "C" fn geometry_set_polygon_attributes(this: Handle, index: i32, direct: f32, reverb: f32, double_sided: bool) -> i32 {
    (n().geometry_set_polygon_attributes)(this.0, index, direct, reverb, double_sided as i32)
}

unsafe extern "C" fn geometry_add_polygon(this: Handle, direct: f32, reverb: f32, double_sided: bool, count: i32, vertices: *const Vector, index: *mut i32) -> i32 {
    (n().geometry_add_polygon)(this.0, direct, reverb, double_sided as i32, count, vertices, index)
}

unsafe extern "C" fn event_set_paused(this: Handle, paused: bool) -> i32 {
    (n().set_paused)(this.0, paused as i32)
}

unsafe extern "C" fn event_get_paused(this: Handle, paused: Out<bool>) -> i32 {
    let mut value = 0i32;
    let result = (n().get_paused)(this.0, &mut value);
    if result == FMOD_OK && !paused.0.is_null() {
        *paused.0 = value != 0;
    }
    result
}

unsafe extern "C" fn event_set_parameter_value(this: Handle, name: Str, value: f32) -> i32 {
    (n().set_parameter_by_name)(this.0, name.0, value, 0)
}

unsafe extern "C" fn description_get_maximum_distance(this: Handle, distance: Out<f32>) -> i32 {
    let mut min = 0.0f32;
    (n().min_max_distance)(this.0, &mut min, distance.0)
}

/// 1.08's `FMOD_STUDIO_CPU_USAGE`: the mixer, streams, geometry, the core's update, Studio's.
unsafe extern "C" fn studio_get_cpu_usage(this: Handle, usage: Quiet<c_void>) -> i32 {
    let (mut studio, mut core) = (0.0f32, [0.0f32; 6]);
    let result = (n().cpu_usage)(this.0, &mut studio, &mut core);
    if result == FMOD_OK && !usage.0.is_null() {
        usage.0.cast::<[f32; 5]>().write_unaligned([core[0], core[1], core[2], core[3], studio]);
    }
    result
}

/// 1.08's `FMOD_STUDIO_ADVANCEDSETTINGS` (cbsize, commandqueuesize, handleinitialsize,
/// studioupdateperiod, idlesampledatapoolsize): what is not 0 goes into 2.03's.
unsafe extern "C" fn studio_set_advanced_settings(this: Handle, settings: Sized) -> i32 {
    let mut s: StudioAdvanced = std::mem::zeroed();
    s.cbsize = size_of::<StudioAdvanced>() as i32;
    let result = (n().studio_get_advanced)(this.0, &mut s);
    if result != FMOD_OK {
        return result;
    }
    if !settings.0.is_null() {
        let w = settings.0.cast::<[u32; 5]>().read_unaligned();
        for (to, from) in [&mut s.commandqueuesize, &mut s.handleinitialsize].into_iter().zip([w[1], w[2]]) {
            if from != 0 {
                *to = from;
            }
        }
        if w[3] != 0 {
            s.studioupdateperiod = w[3] as i32;
        }
        if w[4] != 0 {
            s.idlesampledatapoolsize = w[4] as i32;
        }
    }
    (n().studio_set_advanced)(this.0, &mut s)
}

/// 1.08's `FMOD_ADVANCEDSETTINGS` (0x78 bytes): the game sets `vol0virtualvol` (+0x44), a
/// clock-less run the random seed (+0x74).
unsafe extern "C" fn system_set_advanced_settings(this: Handle, settings: Sized) -> i32 {
    let mut s: Advanced = std::mem::zeroed();
    s.cb_size = size_of::<Advanced>() as i32;
    let result = (n().core_get_advanced)(this.0, &mut s);
    if result != FMOD_OK {
        return result;
    }
    let p = settings.0.cast::<u8>();
    let size = if p.is_null() { 0 } else { p.cast::<i32>().read_unaligned() };
    if size >= 0x78 {
        s.vol0virtualvol = p.add(0x44).cast::<f32>().read_unaligned();
        s.random_seed = p.add(0x74).cast::<u32>().read_unaligned();
    }
    (n().core_set_advanced)(this.0, &mut s)
}

/// A listener never set: 1.08 answers its default as stored, 2.03 turns it right-handed.
unsafe extern "C" fn studio_get_listener_attributes(this: Handle, listener: i32, attributes: Out<Attributes3d>) -> i32 {
    if LISTENERS.load(Ordering::Relaxed) & (1u32 << (listener & 31)) == 0 {
        if !attributes.0.is_null() {
            let mut a = Attributes3d::default();
            a.forward.z = 1.0;
            a.up.y = 1.0;
            *attributes.0 = a;
        }
        return FMOD_OK;
    }
    (n().get_listener)(this.0, listener, attributes.0, std::ptr::null_mut())
}

unsafe extern "C" fn studio_set_listener_attributes(this: Handle, listener: i32, attributes: super::raw::In<Attributes3d>) -> i32 {
    LISTENERS.fetch_or(1u32 << (listener & 31), Ordering::Relaxed);
    (n().set_listener)(this.0, listener, attributes.0, std::ptr::null())
}
