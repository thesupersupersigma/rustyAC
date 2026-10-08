// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! AC's `AudioEngine` with its `AudioEvent`, `AudioReverb` and `AudioOccluder` (AudioEngine.obj
//! of ksAudioFMOD.lib): the Studio system and its settings, the GUID tables, reference-counted
//! banks, the list of live events that `update` runs over every frame, the pool of surface
//! events, the listener and the master volume ramp.
//!
//! Every FMOD call is made where the game makes it, in the game's order, with the game's
//! arguments: the call log of this file is compared with the game's own, call by call.
//! An FMOD error never changes what the game does next (it prints a line); neither here.

use std::collections::BTreeMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::{Path, PathBuf};

use rustyac_physics::data::ini::IniReader;

use crate::fmod::raw::{self as f, Handle, In, Out, OutHandle, Quiet, Sized, Str};
use crate::fmod::types::*;

/// Row-major 4x4, as the game's `mat44f`: row 1 up, row 2 back, row 3 position.
pub type Mat = [f32; 16];
pub type Vec3 = [f32; 3];

/// Index of an `AudioEvent` in the engine (the game passes pointers).
pub type EventId = usize;

/// `EventReverbResponse`
pub const REVERB_OFF: i32 = 0;
pub const REVERB_ON: i32 = 1;

/// AC's `AudioEvent` (0x48 bytes).
#[derive(Debug)]
pub struct AudioEvent {
    description: RawHandle,
    instance: RawHandle,
    pub base_volume: f32,
    pub base_pitch: f32,
    pub path: String,
    reverb_response: i32,
}

struct BankEntry {
    path: String,
    guid: Guid,
    bank: RawHandle,
    ref_count: i32,
}

/// Where the engine finds the files the game reads relative to its folder and to Documents.
#[derive(Clone, Debug)]
pub struct EngineFiles {
    /// The folder `content/sfx/GUIDs.txt` and the cars' `sfx/GUIDs.txt` are read from: the
    /// Assetto Corsa folder.
    pub content_root: PathBuf,
    /// `system/cfg/audio_engine.ini`
    pub audio_engine_ini: PathBuf,
    /// Documents `cfg/audio.ini`
    pub audio_ini: PathBuf,
}

impl EngineFiles {
    /// The player's own files: the AC folder and `Documents\Assetto Corsa`.
    pub fn of_install(ac_root: &Path, documents: &Path) -> EngineFiles {
        EngineFiles {
            content_root: ac_root.to_path_buf(),
            audio_engine_ini: ac_root.join("system/cfg/audio_engine.ini"),
            audio_ini: documents.join("cfg/audio.ini"),
        }
    }
}

/// AC's `AudioEngine` (0x78 bytes).
pub struct AudioEngine {
    system: RawHandle,
    low: RawHandle,
    master_volume: f32,
    target_volume: f32,
    /// `registeredGuids`: lower-case path -> GUID
    guids: BTreeMap<String, Guid>,
    /// `loadedBanks`
    banks: Vec<BankEntry>,
    /// The events themselves (the game's are members of their owners).
    events: Vec<Option<AudioEvent>>,
    /// `registeredEvents`: creation order; `update` walks it.
    registered: Vec<EventId>,
    start_up_time: f32,
    time_from_start: f32,
    /// `freeEvents`: path as given -> events of the pool nobody holds
    free_events: BTreeMap<String, Vec<EventId>>,
    /// `cachedEvents`
    cached: Vec<EventId>,
    /// `reverbValue`: written by `TrackAudio::render`, read by `CarAudioFMOD`.
    pub reverb_value: f32,
    playing: bool,
    pub files: EngineFiles,
    /// The FMOD version found (the game wants [`FMOD_VERSION`] and only complains otherwise).
    pub version: u32,
    verbose: bool,
}

// SAFETY: the FMOD objects are only used through `&mut AudioEngine`, on one thread at a time.
unsafe impl Send for AudioEngine {}

/// `tolower` of the C locale on each character, as the game lower-cases its map keys.
fn lower(text: &str) -> String {
    text.chars().map(|c| c.to_ascii_lowercase()).collect()
}

/// `Path::getFileName`: what follows the last `/` or `\`.
fn file_name(path: &str) -> &str {
    match path.rfind(['/', '\\']) {
        Some(at) => &path[at + 1..],
        None => path,
    }
}

/// `Path::getFileNameWithoutExtension` @ 0x1402309b0: the file name up to its last `.`.
fn file_name_without_extension(path: &str) -> &str {
    let name = file_name(path);
    match name.rfind('.') {
        Some(at) => &name[..at],
        None => name,
    }
}

/// `_fdtest(&x) <= 0`: zero, normal or subnormal.
#[inline]
pub fn fin(x: f32) -> bool {
    x.is_finite()
}

impl AudioEngine {
    /// The game's "FMOD call error" line. Nothing else happens on an error.
    fn chk(&self, what: &str, result: i32) {
        if result != FMOD_OK && self.verbose {
            eprintln!("Audio FMOD call error ({what}): result {result}");
        }
    }

    /// `AudioEngine::AudioEngine` @ 0x1401f6bf0. The FMOD DLLs must be loaded
    /// ([`crate::fmod::raw::load`]).
    pub fn new(files: EngineFiles) -> Result<AudioEngine, String> {
        if f::api().is_none() {
            return Err("FMOD is not loaded".to_string());
        }
        let mut e = AudioEngine {
            system: std::ptr::null_mut(),
            low: std::ptr::null_mut(),
            master_volume: 0.0,
            target_volume: 1.0,
            guids: BTreeMap::new(),
            banks: Vec::new(),
            events: Vec::new(),
            registered: Vec::new(),
            start_up_time: 0.9,
            time_from_start: 0.0,
            free_events: BTreeMap::new(),
            cached: Vec::new(),
            reverb_value: 0.0,
            playing: true,
            files,
            version: 0,
            verbose: std::env::var_os("RUSTYAC_AUDIO_VERBOSE").is_some(),
        };
        let ini1 = IniReader::load(&e.files.audio_ini)?;
        let driver_name = if ini1.has_key("SETTINGS", "DRIVER_NAME") {
            ini1.get_string("SETTINGS", "DRIVER_NAME")
        } else {
            String::new()
        };
        let ini2 = IniReader::load(&e.files.audio_engine_ini)?;
        let live_update = ini2.get_int("SETTINGS", "LIVE_UPDATE")? != 0;
        let max_channels = ini2.get_int("SETTINGS", "MAX_CHANNELS")?;
        e.start_up_time = 0.9;
        e.time_from_start = 0.0;
        // SAFETY: FMOD calls with valid out-pointers; the objects are FMOD's own.
        unsafe {
            let r = f::studio_create(OutHandle(&mut e.system), FMOD_VERSION);
            e.chk("Studio::System::create", r);
            if r != FMOD_OK || e.system.is_null() {
                return Err(format!("FMOD Studio could not be created (result {r})"));
            }
            // the driver named in audio.ini, looked for before the system is initialised
            let mut i = 0;
            loop {
                if i >= e.driver_num() {
                    break;
                }
                if e.driver_info(i) == driver_name {
                    e.set_driver(i);
                }
                i += 1;
            }
            // FMOD_STUDIO_ADVANCEDSETTINGS: cbSize, commandQueueSize, three zeros
            let mut studio_settings = [0x14i32, 0x80000, 0, 0, 0];
            // FMOD_ADVANCEDSETTINGS: 0x78 bytes, only vol0virtualvol (+0x44) = 0.0005
            let mut low_settings = [0u32; 0x78 / 4];
            low_settings[0] = 0x78;
            low_settings[0x44 / 4] = 0x3a03_126f;
            let r = f::studio_get_low_level_system(Handle(e.system), OutHandle(&mut e.low));
            e.chk("getLowLevelSystem", r);
            let r = f::studio_set_advanced_settings(Handle(e.system), Sized(studio_settings.as_mut_ptr().cast()));
            e.chk("Studio setAdvancedSettings", r);
            let r = f::system_set_advanced_settings(Handle(e.low), Sized(low_settings.as_mut_ptr().cast()));
            e.chk("setAdvancedSettings", r);
            let r = f::system_set_software_channels(Handle(e.low), max_channels);
            e.chk("setSoftwareChannels", r);
            let (mut rate, mut mode, mut raw) = (0i32, 0i32, 0i32);
            f::system_get_software_format(Handle(e.low), Quiet(&mut rate), Quiet(&mut mode), Quiet(&mut raw));
            // 0x800 channels, live update when asked for, FMOD_INIT_3D_RIGHTHANDED
            let r = f::studio_initialize(
                Handle(e.system),
                0x800,
                if live_update { STUDIO_INIT_LIVEUPDATE } else { STUDIO_INIT_NORMAL },
                4,
                std::ptr::null_mut(),
            );
            e.chk("Studio initialize", r);
            if r != FMOD_OK {
                return Err(format!("FMOD Studio could not be initialised (result {r})"));
            }
            let r = f::studio_register_plugin(Handle(e.system), crate::dsp::distance_filter::description());
            e.chk("registerPlugin distance filter", r);
            let r = f::studio_register_plugin(Handle(e.system), crate::dsp::gain::description());
            e.chk("registerPlugin gain", r);
            let r = f::system_set_geometry_settings(Handle(e.low), 5000.0);
            e.chk("setGeometrySettings", r);
            e.list_drivers();
            let r = f::system_set_callback(Handle(e.low), Quiet(fmod_callback as *mut c_void), 2);
            e.chk("setCallback", r);
            let (mut length, mut count) = (0u32, 0i32);
            let r = f::system_get_dsp_buffer_size(Handle(e.low), Quiet(&mut length), Quiet(&mut count));
            e.chk("getDSPBufferSize", r);
            let mut version = 0u32;
            let r = f::system_get_version(Handle(e.low), Out(&mut version));
            e.chk("getVersion", r);
            e.version = version;
            if version != FMOD_VERSION && e.verbose {
                eprintln!("Audio error: FMOD version {version:#x}, the game requires {FMOD_VERSION:#x}");
            }
        }
        e.parse_guids("content/sfx/GUIDs.txt")?;
        e.add_bank_ref("content/sfx/common.bank");
        e.playing = true;
        Ok(e)
    }

    /// `getDriverNum` @ 0x1401f9830
    unsafe fn driver_num(&self) -> i32 {
        let mut low = std::ptr::null_mut();
        f::studio_get_low_level_system(Handle(self.system), OutHandle(&mut low));
        let mut n = 0i32;
        let r = f::system_get_num_drivers(Handle(low), Quiet(&mut n));
        self.chk("getNumDrivers", r);
        n
    }

    /// `AudioEngine::getDriverInfo` @ 0x1401f96d0: the device's name.
    unsafe fn driver_info(&self, i: i32) -> String {
        let mut low = std::ptr::null_mut();
        f::studio_get_low_level_system(Handle(self.system), OutHandle(&mut low));
        let (name, _, _, _, r) = driver_info_raw(low, i);
        self.chk("getDriverInfo", r);
        name
    }

    /// `AudioEngine::setDriver` @ 0x1401fb7f0
    unsafe fn set_driver(&self, id: i32) {
        let mut low = std::ptr::null_mut();
        f::studio_get_low_level_system(Handle(self.system), OutHandle(&mut low));
        f::system_set_driver(Handle(low), id);
        let (_, rate, mode, channels, r) = driver_info_raw(low, id);
        self.chk("getDriverInfo", r);
        let r = f::system_set_software_format(Handle(low), rate, mode, channels);
        self.chk("setSoftwareFormat", r);
        let (mut rate, mut mode, mut channels) = (0i32, 0i32, 0i32);
        f::system_get_software_format(Handle(low), Quiet(&mut rate), Quiet(&mut mode), Quiet(&mut channels));
    }

    /// `listDrivers` @ 0x1401fa320 (the game prints the list)
    unsafe fn list_drivers(&self) {
        let mut n = 0i32;
        let r = f::system_get_num_drivers(Handle(self.low), Quiet(&mut n));
        self.chk("getNumDrivers", r);
        for i in 0..n {
            let (_, _, _, _, r) = driver_info_raw(self.low, i);
            self.chk("getDriverInfo", r);
        }
        let mut current = 0i32;
        let r = f::system_get_driver(Handle(self.low), Quiet(&mut current));
        self.chk("getDriver", r);
    }

    /// The names of the output devices, for `--list-audio-devices`.
    pub fn driver_names(&self) -> Vec<String> {
        // SAFETY: as above.
        unsafe {
            let fns = &f::api().expect("FMOD is loaded").fns;
            let mut n = 0i32;
            (fns.system_get_num_drivers)(Handle(self.low), Quiet(&mut n));
            (0..n).map(|i| driver_info_unlogged(self.low, i)).collect()
        }
    }

    /// `AudioEngine::parseGUIDs` @ 0x1401fa8c0: lines `{guid} path` into the table. `file` is
    /// relative to the game folder.
    pub fn parse_guids(&mut self, file: &str) -> Result<(), String> {
        let path = self.files.content_root.join(file);
        // the game would spin for ever on a missing file; its callers check first
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        // a wifstream in the C locale: one byte, one character; text mode
        let text: String = bytes.iter().filter(|&&b| b != b'\r').map(|&b| b as char).collect();
        for line in text.split('\n') {
            let line = line.trim_matches(|c: char| c == ' ' || c == '\t' || c == '\r' || c == '\n');
            if line.is_empty() {
                continue;
            }
            let p: Vec<u32> = line.chars().map(|c| c as u32).collect();
            if p.len() < 39 {
                // undefined in the game (it reads past the line)
                continue;
            }
            let hex = |c: u32| -> u32 {
                if (0x30..=0x39).contains(&c) {
                    c - 0x30
                } else {
                    c.wrapping_sub(0x57)
                }
            };
            let acc = |lo: usize, hi: usize| -> u32 { (lo..hi).fold(0u32, |v, i| (v << 4) | hex(p[i])) };
            let d0 = acc(1, 9);
            let w3 = acc(15, 19);
            let w2 = acc(10, 14);
            let d1 = (w3 << 16) | w2;
            let a = acc(20, 24);
            let b = acc(25, 29);
            let d2 = ((a << 16) | b).swap_bytes();
            let d3 = acc(29, 37).swap_bytes();
            let key: String = line.chars().skip(39).map(|c| c.to_ascii_lowercase()).collect();
            let words = [d0, d1, d2, d3];
            let mut raw = [0u8; 16];
            for (i, w) in words.iter().enumerate() {
                raw[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
            }
            self.guids.insert(key, guid_from_bytes(&raw));
        }
        Ok(())
    }

    /// `AudioEngine::lookupGUID` @ 0x1401fa6c0: an unknown path gets (and keeps) a zero GUID.
    fn lookup_guid(&mut self, path: &str) -> Guid {
        *self.guids.entry(lower(path)).or_default()
    }

    /// `AudioEngine::hasEvent` @ 0x1401f9c60
    pub fn has_event(&self, path: &str) -> bool {
        self.guids.contains_key(&lower(path))
    }

    /// `AudioEngine::addBankRef` @ 0x1401f8220. `path` is relative to the game folder.
    pub fn add_bank_ref(&mut self, path: &str) {
        // not lower-cased, although the table's keys are
        let guid_path = format!("bank:/{}", file_name_without_extension(path));
        if !self.guids.contains_key(&guid_path) && self.verbose {
            eprintln!("Audio error: the bank {path} was not present in any registered GUID file");
        }
        let guid = *self.guids.entry(guid_path).or_default();
        for entry in self.banks.iter_mut() {
            if entry.guid == guid {
                entry.ref_count += 1;
                return;
            }
        }
        let mut entry = BankEntry { path: path.to_string(), guid, bank: std::ptr::null_mut(), ref_count: 0 };
        let name = CString::new(path).unwrap_or_default();
        // SAFETY: a terminated file name and a valid out-pointer.
        let r = unsafe { f::studio_load_bank_file(Handle(self.system), Str(name.as_ptr()), 0, OutHandle(&mut entry.bank)) };
        self.chk("loadBankFile", r);
        entry.ref_count = 1;
        self.banks.push(entry);
    }

    /// Did the bank of this path load? (Not a function of the game: for the survey.)
    pub fn bank_loaded(&self, path: &str) -> bool {
        self.banks.iter().any(|b| b.path == path && !b.bank.is_null())
    }

    /// `AudioEngine::removeBankRef` @ 0x1401fb200
    pub fn remove_bank_ref(&mut self, path: &str) {
        for i in 0..self.banks.len() {
            if self.banks[i].path == path {
                self.banks[i].ref_count -= 1;
                if self.banks[i].ref_count != 0 {
                    return;
                }
                // SAFETY: the bank FMOD handed out (the game does not test it for null either).
                unsafe { f::bank_unload(Handle(self.banks[i].bank)) };
                self.banks.remove(i);
                return;
            }
        }
    }

    // --- AudioEvent ------------------------------------------------------------------------------

    fn ev(&self, e: EventId) -> &AudioEvent {
        self.events[e].as_ref().expect("a live event")
    }

    /// `AudioEvent::AudioEvent(engine, path, reverbResponse)` @ 0x1401f7630
    pub fn create_event(&mut self, path: &str, reverb_response: i32) -> EventId {
        if !self.has_event(path) && self.verbose {
            eprintln!("Audio error: trying to load an FMOD event that does not exist: {path}");
        }
        let guid = self.lookup_guid(path);
        let mut event = AudioEvent {
            description: std::ptr::null_mut(),
            instance: std::ptr::null_mut(),
            base_volume: 1.0,
            base_pitch: 1.0,
            path: path.to_string(),
            reverb_response,
        };
        // SAFETY: valid out-pointers; all three calls are made whatever the one before said.
        unsafe {
            let r = f::studio_get_event_by_id(Handle(self.system), In(&guid), OutHandle(&mut event.description));
            self.chk("getEventByID", r);
            let r = f::description_load_sample_data(Handle(event.description));
            self.chk("loadSampleData", r);
            let r = f::description_create_instance(Handle(event.description), OutHandle(&mut event.instance));
            self.chk("createInstance", r);
        }
        let id = self.events.len();
        self.events.push(Some(event));
        self.registered.push(id);
        id
    }

    /// Does the event have an FMOD instance? (False for a path FMOD does not know.)
    pub fn event_is_valid(&self, e: EventId) -> bool {
        !self.ev(e).instance.is_null()
    }

    /// `AudioEvent::~AudioEvent` @ 0x1401f7b30
    pub fn destroy_event(&mut self, e: EventId) {
        let Some(event) = self.events[e].take() else { return };
        if !event.instance.is_null() {
            // SAFETY: the event's own FMOD objects.
            unsafe {
                let r = f::event_stop(Handle(event.instance), STOP_ALLOWFADEOUT);
                self.chk("stop", r);
                self.registered.retain(|&id| id != e);
                let r = f::event_release(Handle(event.instance));
                self.chk("release", r);
                let r = f::description_unload_sample_data(Handle(event.description));
                self.chk("unloadSampleData", r);
            }
        }
    }

    /// `AudioEvent::start` @ 0x1401fbf40
    pub fn event_start(&self, e: EventId) {
        let instance = Handle(self.ev(e).instance);
        // SAFETY: FMOD calls on the event's instance (FMOD rejects a null one).
        unsafe {
            let r = f::event_set_timeline_position(instance, 0);
            self.chk("setTimelinePosition", r);
            let r = f::event_start(instance);
            self.chk("start", r);
        }
    }

    /// `AudioEvent::stop` @ 0x1401fc040 (always "allow fade out")
    pub fn event_stop(&self, e: EventId) {
        // SAFETY: as above.
        let r = unsafe { f::event_stop(Handle(self.ev(e).instance), STOP_ALLOWFADEOUT) };
        self.chk("stop", r);
    }

    /// `AudioEvent::resume` @ 0x1401fb320
    pub fn event_resume(&self, e: EventId, resume: bool) {
        // SAFETY: as above.
        let r = unsafe { f::event_set_paused(Handle(self.ev(e).instance), !resume) };
        self.chk("setPaused", r);
    }

    /// `AudioEvent::isPlaying` @ 0x1401fa170: not stopped and not stopping.
    pub fn event_is_playing(&self, e: EventId) -> bool {
        let mut state = 0i32;
        // SAFETY: as above.
        let r = unsafe { f::event_get_playback_state(Handle(self.ev(e).instance), Out(&mut state)) };
        self.chk("getPlaybackState", r);
        (state.wrapping_sub(2) as u32 & 0xffff_fffd) != 0
    }

    /// `AudioEvent::isPaused` @ 0x1401fa120
    pub fn event_is_paused(&self, e: EventId) -> bool {
        let mut paused = false;
        // SAFETY: as above.
        let r = unsafe { f::event_get_paused(Handle(self.ev(e).instance), Out(&mut paused)) };
        self.chk("getPaused", r);
        paused
    }

    /// `AudioEvent::isWithinRange` @ 0x1401fa1c0
    pub fn event_is_within_range(&self, e: EventId) -> bool {
        let event = self.ev(e);
        let mut l = Attributes3d::default();
        let mut a = Attributes3d::default();
        let mut rolloff = 0.0f32;
        let mut max_distance = 0.0f32;
        // SAFETY: valid out-pointers.
        unsafe {
            let r = f::studio_get_listener_attributes(Handle(self.system), 0, Out(&mut l));
            self.chk("getListenerAttributes", r);
            let r = f::system_get_3d_settings(Handle(self.low), Out(std::ptr::null_mut()), Out(std::ptr::null_mut()), Out(&mut rolloff));
            self.chk("get3DSettings", r);
            let r = f::event_get_3d_attributes(Handle(event.instance), Out(&mut a));
            self.chk("get3DAttributes", r);
            let r = f::description_get_maximum_distance(Handle(event.description), Out(&mut max_distance));
            self.chk("getMaximumDistance", r);
        }
        let dx = l.position.x - a.position.x;
        let dy = l.position.y - a.position.y;
        let dz = l.position.z - a.position.z;
        let d2 = ((dy * dy) + (dx * dx)) + (dz * dz);
        let dist = d2.sqrt() * rolloff;
        max_distance >= dist
    }

    /// `AudioEvent::setParameter` @ 0x1401fbc40
    pub fn event_set_parameter(&self, e: EventId, name: &CStr, value: f32) {
        if fin(value) {
            // SAFETY: a terminated name.
            let r = unsafe { f::event_set_parameter_value(Handle(self.ev(e).instance), Str(name.as_ptr()), value) };
            self.chk("setParameterValue", r);
        } else if self.verbose {
            eprintln!("Audio error: setParameter({name:?}) called with {value}, event: {}", self.ev(e).path);
        }
    }

    /// `AudioEvent::set3DAttributes(const mat44f&, const vec3f&)` @ 0x1401fb370
    pub fn event_set_3d(&self, e: EventId, m: &Mat, velocity: &Vec3) {
        if !m.iter().all(|x| fin(*x)) || !velocity.iter().all(|x| fin(*x)) {
            if self.verbose {
                eprintln!("Audio error: set3DAttributes called with a value that is not finite, event: {}", self.ev(e).path);
            }
            return;
        }
        let a = attributes_of(m, velocity);
        // SAFETY: plain data in.
        let r = unsafe { f::event_set_3d_attributes(Handle(self.ev(e).instance), In(&a)) };
        self.chk("set3DAttributes", r);
    }

    /// `AudioEvent::set3DAttributes()` @ 0x1401fb520: the event sits on the listener.
    pub fn event_set_3d_at_listener(&self, e: EventId) {
        let mut a = Attributes3d::default();
        // SAFETY: valid pointers.
        unsafe {
            let r = f::studio_get_listener_attributes(Handle(self.system), 0, Out(&mut a));
            self.chk("getListenerAttributes", r);
            let r = f::event_set_3d_attributes(Handle(self.ev(e).instance), In(&a));
            self.chk("set3DAttributes", r);
        }
    }

    /// `AudioEvent::setBaseVolume` @ 0x1401fb6d0
    pub fn event_set_base_volume(&mut self, e: EventId, volume: f32) {
        if fin(volume) {
            let event = self.events[e].as_mut().expect("a live event");
            event.base_volume = volume;
            let instance = event.instance;
            // SAFETY: the event's instance.
            let r = unsafe { f::event_set_volume(Handle(instance), volume) };
            self.chk("setVolume", r);
        } else if self.verbose {
            eprintln!("Audio error: setBaseVolume called with {volume}, event: {}", self.ev(e).path);
        }
    }

    /// `AudioEvent::setBasePitch` @ 0x1401fb610
    pub fn event_set_base_pitch(&mut self, e: EventId, pitch: f32) {
        if fin(pitch) {
            let event = self.events[e].as_mut().expect("a live event");
            event.base_pitch = pitch;
            let instance = event.instance;
            // SAFETY: the event's instance.
            let r = unsafe { f::event_set_pitch(Handle(instance), pitch) };
            self.chk("setPitch", r);
        } else if self.verbose {
            eprintln!("Audio error: setBasePitch called with {pitch}, event: {}", self.ev(e).path);
        }
    }

    pub fn event_path(&self, e: EventId) -> &str {
        &self.ev(e).path
    }

    // --- listener ----------------------------------------------------------------------------------

    /// `AudioEngine::setListener` @ 0x1401fbb30
    pub fn set_listener(&self, m: &Mat, velocity: &Vec3) {
        let a = attributes_of(m, velocity);
        // SAFETY: plain data in.
        let r = unsafe { f::studio_set_listener_attributes(Handle(self.system), 0, In(&a)) };
        self.chk("setListenerAttributes", r);
    }

    /// `AudioEngine::setDistanceScale` @ 0x1401fb790: FMOD's roll-off scale.
    pub fn set_distance_scale(&self, scale: f32) {
        // SAFETY: plain values.
        let r = unsafe { f::system_set_3d_settings(Handle(self.low), 1.0, 1.0, scale) };
        self.chk("set3DSettings", r);
    }

    /// `AudioEngine::getListenerPosition` @ 0x1401f9880
    pub fn listener_position(&self) -> Vec3 {
        let mut a = Attributes3d::default();
        // SAFETY: a valid out-pointer.
        let r = unsafe { f::studio_get_listener_attributes(Handle(self.system), 0, Out(&mut a)) };
        self.chk("getListenerAttributes", r);
        [a.position.x, a.position.y, a.position.z]
    }

    /// `AudioEngine::listenerDistance` @ 0x1401fa610
    pub fn listener_distance(&self, p: &Vec3) -> f32 {
        let mut a = Attributes3d::default();
        let mut rolloff = 0.0f32;
        // SAFETY: valid out-pointers.
        unsafe {
            let r = f::studio_get_listener_attributes(Handle(self.system), 0, Out(&mut a));
            self.chk("getListenerAttributes", r);
            let r = f::system_get_3d_settings(Handle(self.low), Out(std::ptr::null_mut()), Out(std::ptr::null_mut()), Out(&mut rolloff));
            self.chk("get3DSettings", r);
        }
        let dx = a.position.x - p[0];
        let dy = a.position.y - p[1];
        let dz = a.position.z - p[2];
        (((dy * dy) + (dx * dx)) + (dz * dz)).sqrt()
    }

    /// `AudioEngine::getVolume` @ 0x1401f9c50
    pub fn volume(&self, target: bool) -> f32 {
        if target {
            self.target_volume
        } else {
            self.master_volume
        }
    }

    // --- per frame ---------------------------------------------------------------------------------

    unsafe fn master_group(&self, what: &str) -> RawHandle {
        let mut group = std::ptr::null_mut();
        let r = f::system_get_master_channel_group(Handle(self.low), OutHandle(&mut group));
        self.chk(what, r);
        group
    }

    /// `AudioEngine::rampVolume` @ 0x1401fafa0
    #[allow(clippy::float_cmp)]
    fn ramp_volume(&mut self, dt: f32) {
        let tgt = self.target_volume;
        // ucomiss ; je: equal or unordered skips
        if tgt < self.master_volume || tgt > self.master_volume {
            let t = self.time_from_start;
            let v = (t * tgt) / self.start_up_time;
            self.master_volume = if v > tgt {
                tgt
            } else if !(v >= 0.0) {
                0.0
            } else {
                v
            };
            self.time_from_start = t + dt;
        }
        // SAFETY: FMOD calls.
        unsafe {
            let group = self.master_group("getMasterChannelGroup");
            let r = f::channel_set_volume(Handle(group), self.master_volume);
            self.chk("ChannelControl::setVolume", r);
        }
    }

    /// `AudioEngine::update` @ 0x1401fc080: the volume ramp, then for every live event the
    /// occlusion between it and the listener as its volume, then FMOD's own update.
    pub fn update(&mut self, dt: f32) {
        if self.playing && (self.master_volume < self.target_volume || self.master_volume > self.target_volume) {
            self.ramp_volume(dt);
        }
        let mut l = Attributes3d::default();
        // SAFETY: valid pointers; the events' own instances.
        unsafe {
            let r = f::studio_get_listener_attributes(Handle(self.system), 0, Out(&mut l));
            self.chk("getListenerAttributes", r);
            for &id in &self.registered {
                let event = self.ev(id);
                let base = event.base_volume;
                let mut a = Attributes3d::default();
                let r = f::event_get_3d_attributes(Handle(event.instance), Out(&mut a));
                self.chk("get3DAttributes", r);
                let (mut direct, mut reverb) = (0.0f32, 0.0f32);
                let r = f::system_get_geometry_occlusion(Handle(self.low), In(&l.position), In(&a.position), Out(&mut direct), Out(&mut reverb));
                self.chk("getGeometryOcclusion", r);
                let r = f::event_set_volume(Handle(event.instance), (1.0 - direct) * base);
                self.chk("setVolume", r);
            }
            let r = f::studio_update(Handle(self.system));
            self.chk("Studio update", r);
            self.check_output_sanity();
        }
    }

    /// `checkOutputSanity` @ 0x1401f8e00 (the game only prints)
    unsafe fn check_output_sanity(&self) {
        let (mut n, mut current, mut output) = (0i32, 0i32, 0i32);
        let r = f::system_get_num_drivers(Handle(self.low), Quiet(&mut n));
        self.chk("getNumDrivers", r);
        let r = f::system_get_driver(Handle(self.low), Quiet(&mut current));
        self.chk("getDriver", r);
        let r = f::system_get_output(Handle(self.low), Quiet(&mut output));
        self.chk("getOutput", r);
    }

    /// `AudioEngine::updateProperties` @ 0x1401fc310: every event that has no reverb response
    /// gets an (almost) dry reverb send. Called at the end of every car's sound constructor.
    pub fn update_properties(&self) {
        // SAFETY: FMOD calls on live events.
        unsafe {
            let r = f::studio_flush_commands(Handle(self.system));
            self.chk("flushCommands", r);
            for &id in &self.registered {
                let event = self.ev(id);
                let mut group: RawHandle = std::ptr::null_mut();
                let r = f::event_get_channel_group(Handle(event.instance), &mut group);
                self.chk("getChannelGroup", r);
                if !group.is_null() && event.reverb_response == REVERB_OFF {
                    let r = f::channel_set_reverb_properties(Handle(group), 0, 0.001);
                    self.chk("setReverbProperties", r);
                }
            }
        }
    }

    /// `AudioEngine::start` @ 0x1401fbec0 (leaving the pause menu, leaving a replay)
    pub fn start(&mut self) {
        // SAFETY: FMOD calls.
        unsafe {
            let group = self.master_group("getMasterChannelGroup");
            let r = f::channel_set_volume(Handle(group), self.master_volume);
            self.chk("ChannelControl::setVolume", r);
        }
        self.playing = true;
    }

    /// `AudioEngine::stop` @ 0x1401fbfc0: the master volume goes to 0; the events run on.
    pub fn stop(&mut self) {
        // SAFETY: FMOD calls.
        unsafe {
            let group = self.master_group("getMasterChannelGroup");
            let r = f::channel_set_volume(Handle(group), 0.0);
            self.chk("ChannelControl::setVolume", r);
        }
        self.playing = false;
    }

    /// `AudioEngine::setVolume` @ 0x1401fbdf0: a new target, reached by a 0.9 s ramp from 0.
    pub fn set_volume(&mut self, volume: f32) {
        self.time_from_start = 0.0;
        self.target_volume = volume;
        // SAFETY: FMOD calls.
        unsafe {
            let group = self.master_group("getMasterChannelGroup");
            let r = f::channel_set_volume(Handle(group), self.target_volume);
            self.chk("ChannelControl::setVolume", r);
        }
        // comiss 0.0,startUpTime ; jb
        self.master_volume = if !(0.0 >= self.start_up_time) { 0.0 } else { volume };
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    // --- the pool of surface events ----------------------------------------------------------------

    /// `AudioEngine::addCache` @ 0x1401f8730: twelve instances of an event, to be handed out.
    pub fn add_cache(&mut self, path: &str) {
        if !self.free_events.entry(path.to_string()).or_default().is_empty() {
            return;
        }
        for _ in 0..12 {
            let id = self.create_event(path, REVERB_OFF);
            self.cached.push(id);
            self.free_events.entry(path.to_string()).or_default().push(id);
        }
    }

    /// `AudioEngine::getCachedEvent` @ 0x1401f9650: the last one put back, or none.
    pub fn get_cached_event(&mut self, path: &str) -> Option<EventId> {
        self.free_events.entry(path.to_string()).or_default().pop()
    }

    /// `AudioEngine::releaseCachedEvent` @ 0x1401fb0d0
    pub fn release_cached_event(&mut self, e: EventId) {
        self.event_stop(e);
        let path = self.ev(e).path.clone();
        self.free_events.entry(path).or_default().push(e);
    }

    /// `AudioEngine::clearCache` @ 0x1401f8f60
    pub fn clear_cache(&mut self) {
        for id in std::mem::take(&mut self.cached) {
            self.destroy_event(id);
        }
        self.free_events.clear();
    }

    pub(crate) fn low(&self) -> RawHandle {
        self.low
    }

    pub(crate) fn system(&self) -> RawHandle {
        self.system
    }

    pub(crate) fn chk_pub(&self, what: &str, result: i32) {
        self.chk(what, result);
    }
}

impl Drop for AudioEngine {
    /// `AudioEngine::~AudioEngine` @ 0x1401f79f0
    fn drop(&mut self) {
        if self.system.is_null() {
            return;
        }
        self.clear_cache();
        self.remove_bank_ref("content/sfx/common.bank");
        // SAFETY: the system this engine created.
        let r = unsafe { f::studio_release(Handle(self.system)) };
        self.chk("Studio release", r);
        self.system = std::ptr::null_mut();
    }
}

/// The game's mapping of a matrix and a velocity to `FMOD_3D_ATTRIBUTES`: position = row 3,
/// forward = row 2 with the sign bits flipped, up = row 1. No normalising.
fn attributes_of(m: &Mat, velocity: &Vec3) -> Attributes3d {
    let flip = |x: f32| f32::from_bits(x.to_bits() ^ 0x8000_0000);
    Attributes3d {
        position: Vector { x: m[12], y: m[13], z: m[14] },
        velocity: Vector { x: velocity[0], y: velocity[1], z: velocity[2] },
        forward: Vector { x: flip(m[8]), y: flip(m[9]), z: flip(m[10]) },
        up: Vector { x: m[4], y: m[5], z: m[6] },
    }
}

fn guid_from_bytes(raw: &[u8; 16]) -> Guid {
    Guid {
        data1: u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]),
        data2: u16::from_le_bytes([raw[4], raw[5]]),
        data3: u16::from_le_bytes([raw[6], raw[7]]),
        data4: [raw[8], raw[9], raw[10], raw[11], raw[12], raw[13], raw[14], raw[15]],
    }
}

/// `System::getDriverInfo` as the game calls it: (name, rate, speaker mode, channels, result).
unsafe fn driver_info_raw(low: RawHandle, i: i32) -> (String, i32, i32, i32, i32) {
    let mut name = [0u8; 0x80];
    let mut guid = Guid::default();
    let (mut rate, mut mode, mut channels) = (0i32, 0i32, 0i32);
    let r = f::system_get_driver_info(
        Handle(low),
        i,
        Quiet(name.as_mut_ptr().cast::<c_char>()),
        0x80,
        Quiet(&mut guid),
        Quiet(&mut rate),
        Quiet(&mut mode),
        Quiet(&mut channels),
    );
    (name_of(&name), rate, mode, channels, r)
}

unsafe fn driver_info_unlogged(low: RawHandle, i: i32) -> String {
    let fns = &f::api().expect("FMOD is loaded").fns;
    let mut name = [0u8; 0x80];
    let mut guid = Guid::default();
    let (mut rate, mut mode, mut channels) = (0i32, 0i32, 0i32);
    (fns.system_get_driver_info)(
        Handle(low),
        i,
        Quiet(name.as_mut_ptr().cast::<c_char>()),
        0x80,
        Quiet(&mut guid),
        Quiet(&mut rate),
        Quiet(&mut mode),
        Quiet(&mut channels),
    );
    name_of(&name)
}

fn name_of(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// `fmodCallback` @ 0x1401f9580: the sound device was lost. The game stops with a "critical
/// error"; rustyAC says so once and carries on silently.
unsafe extern "C" fn fmod_callback(system: RawHandle, kind: u32, _a: *mut c_void, _b: *mut c_void, _c: *mut c_void) -> i32 {
    if kind != 2 {
        return 0;
    }
    let mut driver = 0i32;
    f::c_system_get_driver(Handle(system), Quiet(&mut driver));
    eprintln!("audio: the sound device was lost (current driver {driver})");
    0
}

// --- AudioReverb ---------------------------------------------------------------------------------

/// AC's `AudioReverb`: a sphere in which FMOD mixes a reverb.
pub struct AudioReverb {
    reverb: RawHandle,
    wet_level: f32,
}

/// `presetNames` @ 0x14151cba0
pub const PRESET_NAMES: [&str; 24] = [
    "OFF", "GENERIC", "PADDEDCELL", "ROOM", "BATHROOM", "LIVINGROOM", "STONEROOM", "AUDITORIUM", "CONCERTHALL", "CAVE", "ARENA", "HANGAR",
    "CARPETTEDHALLWAY", "HALLWAY", "STONECORRIDOR", "ALLEY", "FOREST", "CITY", "MOUNTAINS", "QUARRY", "PLAIN", "PARKINGLOT", "SEWERPIPE",
    "UNDERWATER",
];

/// `presetTable` @ 0x1404e81c0: DecayTime, EarlyDelay, LateDelay, HFReference, HFDecayRatio,
/// Diffusion, Density, LowShelfFrequency, LowShelfGain, HighCut, EarlyLateMix, WetLevel.
pub const PRESET_TABLE: [[f32; 12]; 24] = [
    [1000.0, 7.0, 11.0, 5000.0, 100.0, 100.0, 100.0, 250.0, 0.0, 20.0, 96.0, -80.0],
    [1500.0, 7.0, 11.0, 5000.0, 83.0, 100.0, 100.0, 250.0, 0.0, 14500.0, 96.0, -8.0],
    [170.0, 1.0, 2.0, 5000.0, 10.0, 100.0, 100.0, 250.0, 0.0, 160.0, 84.0, -7.8],
    [400.0, 2.0, 3.0, 5000.0, 83.0, 100.0, 100.0, 250.0, 0.0, 6050.0, 88.0, -9.4],
    [1500.0, 7.0, 11.0, 5000.0, 54.0, 100.0, 60.0, 250.0, 0.0, 2900.0, 83.0, 0.5],
    [500.0, 3.0, 4.0, 5000.0, 10.0, 100.0, 100.0, 250.0, 0.0, 160.0, 58.0, -19.0],
    [2300.0, 12.0, 17.0, 5000.0, 64.0, 100.0, 100.0, 250.0, 0.0, 7800.0, 71.0, -8.5],
    [4300.0, 20.0, 30.0, 5000.0, 59.0, 100.0, 100.0, 250.0, 0.0, 5850.0, 64.0, -11.7],
    [3900.0, 20.0, 29.0, 5000.0, 70.0, 100.0, 100.0, 250.0, 0.0, 5650.0, 80.0, -9.8],
    [2900.0, 15.0, 22.0, 5000.0, 100.0, 100.0, 100.0, 250.0, 0.0, 20000.0, 59.0, -11.3],
    [7200.0, 20.0, 30.0, 5000.0, 33.0, 100.0, 100.0, 250.0, 0.0, 4500.0, 80.0, -9.6],
    [10000.0, 20.0, 30.0, 5000.0, 23.0, 100.0, 100.0, 250.0, 0.0, 3400.0, 72.0, -7.4],
    [300.0, 2.0, 30.0, 5000.0, 10.0, 100.0, 100.0, 250.0, 0.0, 500.0, 56.0, -24.0],
    [1500.0, 7.0, 11.0, 5000.0, 59.0, 100.0, 100.0, 250.0, 0.0, 7800.0, 87.0, -5.5],
    [270.0, 13.0, 20.0, 5000.0, 79.0, 100.0, 100.0, 250.0, 0.0, 9000.0, 86.0, -6.0],
    [1500.0, 7.0, 11.0, 5000.0, 86.0, 100.0, 100.0, 250.0, 0.0, 8300.0, 80.0, -9.8],
    [1500.0, 162.0, 88.0, 5000.0, 54.0, 79.0, 100.0, 250.0, 0.0, 760.0, 94.0, -12.3],
    [1500.0, 7.0, 11.0, 5000.0, 67.0, 50.0, 100.0, 250.0, 0.0, 4050.0, 66.0, -26.0],
    [1500.0, 300.0, 100.0, 5000.0, 21.0, 27.0, 100.0, 250.0, 0.0, 1220.0, 82.0, -24.0],
    [1500.0, 61.0, 25.0, 5000.0, 83.0, 100.0, 100.0, 250.0, 0.0, 3400.0, 100.0, -5.0],
    [1500.0, 179.0, 100.0, 5000.0, 50.0, 21.0, 100.0, 250.0, 0.0, 1670.0, 65.0, -28.0],
    [1700.0, 8.0, 12.0, 5000.0, 100.0, 100.0, 100.0, 250.0, 0.0, 20000.0, 56.0, -19.5],
    [2800.0, 14.0, 21.0, 5000.0, 14.0, 80.0, 60.0, 250.0, 0.0, 3400.0, 66.0, 1.2],
    [1500.0, 7.0, 11.0, 5000.0, 10.0, 100.0, 100.0, 250.0, 0.0, 500.0, 92.0, 7.0],
];

fn properties_of(p: &[f32; 12]) -> ReverbProperties {
    ReverbProperties {
        decay_time: p[0],
        early_delay: p[1],
        late_delay: p[2],
        hf_reference: p[3],
        hf_decay_ratio: p[4],
        diffusion: p[5],
        density: p[6],
        low_shelf_frequency: p[7],
        low_shelf_gain: p[8],
        high_cut: p[9],
        early_late_mix: p[10],
        wet_level: p[11],
    }
}

impl AudioReverb {
    /// `AudioReverb::AudioReverb` @ 0x1401f7860
    pub fn new(engine: &AudioEngine) -> AudioReverb {
        let mut reverb = std::ptr::null_mut();
        // SAFETY: a valid out-pointer.
        let r = unsafe { f::system_create_reverb_3d(Handle(engine.low()), OutHandle(&mut reverb)) };
        engine.chk_pub("createReverb3D", r);
        AudioReverb { reverb, wet_level: 0.0 }
    }

    /// The move constructor @ 0x1401f7840 does not carry the wet level over.
    pub fn moved(mut self) -> AudioReverb {
        self.wet_level = 0.0;
        self
    }

    /// `AudioReverb::enable` @ 0x1401f9030
    pub fn enable(&self, engine: &AudioEngine, on: bool) {
        // SAFETY: the reverb FMOD handed out.
        let r = unsafe { f::reverb_set_active(Handle(self.reverb), on) };
        engine.chk_pub("Reverb3D::setActive", r);
    }

    /// `AudioReverb::set3DAttributes` @ 0x1401fb5a0
    pub fn set_3d(&self, engine: &AudioEngine, position: &Vec3, min_distance: f32, max_distance: f32) {
        let v = Vector { x: position[0], y: position[1], z: position[2] };
        // SAFETY: plain data in.
        let r = unsafe { f::reverb_set_3d_attributes(Handle(self.reverb), In(&v), min_distance, max_distance) };
        engine.chk_pub("Reverb3D::set3DAttributes", r);
    }

    /// `AudioReverb::setPreset` @ 0x1401fbd10 (the wet level member stays as it was)
    pub fn set_preset(&self, engine: &AudioEngine, preset: usize) {
        let p = properties_of(&PRESET_TABLE[preset]);
        // SAFETY: plain data in.
        let r = unsafe { f::reverb_set_properties(Handle(self.reverb), In(&p)) };
        engine.chk_pub("Reverb3D::setProperties", r);
    }

    /// `AudioReverb::setProperties` @ 0x1401fbd70
    pub fn set_properties(&mut self, engine: &AudioEngine, p: &[f32; 12]) {
        let local = properties_of(p);
        self.wet_level = p[11];
        // SAFETY: plain data in.
        let r = unsafe { f::reverb_set_properties(Handle(self.reverb), In(&local)) };
        engine.chk_pub("Reverb3D::setProperties", r);
    }

    /// `AudioReverb::getPresetFromName` @ 0x1401f9a40: an unknown name is preset 0.
    pub fn preset_from_name(name: &str) -> usize {
        PRESET_NAMES.iter().position(|n| *n == name).unwrap_or(0)
    }

    /// `AudioReverb::hearValue` @ 0x1401f9dd0: how much of this reverb the listener is in.
    pub fn hear_value(&self, engine: &AudioEngine) -> f32 {
        let mut l = Attributes3d::default();
        let mut rolloff = 0.0f32;
        let mut p = Vector::default();
        let (mut min_distance, mut max_distance) = (0.0f32, 0.0f32);
        // SAFETY: valid out-pointers.
        unsafe {
            let r = f::studio_get_listener_attributes(Handle(engine.system()), 0, Out(&mut l));
            engine.chk_pub("getListenerAttributes", r);
            let r = f::system_get_3d_settings(Handle(engine.low()), Out(std::ptr::null_mut()), Out(std::ptr::null_mut()), Out(&mut rolloff));
            engine.chk_pub("get3DSettings", r);
            let r = f::reverb_get_3d_attributes(Handle(self.reverb), Out(&mut p), Out(&mut min_distance), Out(&mut max_distance));
            engine.chk_pub("Reverb3D::get3DAttributes", r);
        }
        let dx = l.position.x - p.x;
        let dy = l.position.y - p.y;
        let dz = l.position.z - p.z;
        let d = (((dy * dy) + (dx * dx)) + (dz * dz)).sqrt();
        if d >= max_distance {
            return 0.0;
        }
        let w = (self.wet_level + 80.0) * 0.01;
        let c = if w > 1.0 {
            1.0
        } else if !(w >= 0.0) {
            0.0
        } else {
            w
        };
        (1.0 - (d / max_distance)) * c
    }

    /// `AudioReverb::~AudioReverb` @ 0x1401f7c60
    pub fn release(&mut self, engine: &AudioEngine) {
        if !self.reverb.is_null() {
            // SAFETY: the reverb FMOD handed out.
            let r = unsafe { f::reverb_release(Handle(self.reverb)) };
            engine.chk_pub("Reverb3D::release", r);
            self.reverb = std::ptr::null_mut();
        }
    }
}

// --- AudioOccluder -------------------------------------------------------------------------------

/// AC's `AudioOccluder`: triangles that muffle what is behind them.
pub struct AudioOccluder {
    geometry: RawHandle,
    volume_occlusion: f32,
    reverb_occlusion: f32,
    double_sided: bool,
}

impl AudioOccluder {
    /// `AudioOccluder::AudioOccluder` @ 0x1401f77e0
    pub fn new(engine: &AudioEngine, max_polygons: i32, max_vertices: i32) -> AudioOccluder {
        let mut geometry = std::ptr::null_mut();
        // SAFETY: a valid out-pointer.
        let r = unsafe { f::system_create_geometry(Handle(engine.low()), max_polygons, max_vertices, OutHandle(&mut geometry)) };
        engine.chk_pub("createGeometry", r);
        AudioOccluder { geometry, volume_occlusion: 0.0, reverb_occlusion: 0.0, double_sided: true }
    }

    /// `AudioOccluder::enable` @ 0x1401f8ff0
    pub fn enable(&self, engine: &AudioEngine, on: bool) {
        // SAFETY: the geometry FMOD handed out.
        let r = unsafe { f::geometry_set_active(Handle(self.geometry), on) };
        engine.chk_pub("Geometry::setActive", r);
    }

    /// `AudioOccluder::addTriangle` @ 0x1401f8890
    pub fn add_triangle(&self, engine: &AudioEngine, a: &Vec3, b: &Vec3, c: &Vec3) {
        let v = [Vector { x: a[0], y: a[1], z: a[2] }, Vector { x: b[0], y: b[1], z: b[2] }, Vector { x: c[0], y: c[1], z: c[2] }];
        let mut index = 0i32;
        // SAFETY: three vertices and a valid out-pointer.
        let r = unsafe { f::geometry_add_polygon(Handle(self.geometry), self.volume_occlusion, self.reverb_occlusion, self.double_sided, 3, v.as_ptr(), &mut index) };
        engine.chk_pub("Geometry::addPolygon", r);
    }

    /// `AudioOccluder::setDoubleSided` @ 0x1401fb7e0
    pub fn set_double_sided(&mut self, engine: &AudioEngine, on: bool) {
        self.double_sided = on;
        self.update_polygon_attributes(engine);
    }

    /// `AudioOccluder::setVolumeOcclusion` @ 0x1401fbeb0
    pub fn set_volume_occlusion(&mut self, engine: &AudioEngine, volume: f32) {
        self.volume_occlusion = volume;
        self.update_polygon_attributes(engine);
    }

    /// `AudioOccluder::updatePolygonAttributes` @ 0x1401fc260
    fn update_polygon_attributes(&self, engine: &AudioEngine) {
        let mut n = 0i32;
        // SAFETY: the geometry FMOD handed out.
        unsafe {
            let r = f::geometry_get_num_polygons(Handle(self.geometry), Out(&mut n));
            engine.chk_pub("Geometry::getNumPolygons", r);
            for i in 0..n {
                let r = f::geometry_set_polygon_attributes(Handle(self.geometry), i, self.volume_occlusion, self.reverb_occlusion, self.double_sided);
                engine.chk_pub("Geometry::setPolygonAttributes", r);
            }
        }
    }

    /// `AudioOccluder::~AudioOccluder` @ 0x1401f7c20
    pub fn release(&mut self, engine: &AudioEngine) {
        if !self.geometry.is_null() {
            // SAFETY: the geometry FMOD handed out.
            let r = unsafe { f::geometry_release(Handle(self.geometry)) };
            engine.chk_pub("Geometry::release", r);
            self.geometry = std::ptr::null_mut();
        }
    }
}
