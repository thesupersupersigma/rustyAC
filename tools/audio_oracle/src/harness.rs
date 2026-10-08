// SPDX-License-Identifier: GPL-3.0-or-later

//! The game's side of the audio oracle: acs.exe's own `AudioEngine`, `TrackAudio`,
//! `CarAudioFMOD` and the few `CarAvatar` functions the sound hangs on, called by address on
//! objects laid out by hand. The game's FMOD imports are bound to the recording layer of
//! `rustyac-audio`, which forwards to the real FMOD DLLs.
//!
//! What the harness itself does in place of game code it cannot run (the whole `Sim`, `Game`
//! and `CarAvatar`) is small and is listed in `docs/port/audio.md`: the frame's order of calls,
//! `Sim::stepPhysicsEvent`'s hand-over of collisions, the listener ranking of `Sim::update`,
//! and `Sim::loadTrack`'s loop over the surfaces.

use std::path::Path;

use rustyac_audio::fmod::log;
use rustyac_audio::track::Scene;

use crate::acs::Acs;
use crate::drive::{Drive, DT};

const VA_AUDIO_ENGINE_CTOR: usize = 0x1_401f_6bf0; // AudioEngine::AudioEngine()
const VA_AUDIO_ENGINE_DTOR: usize = 0x1_401f_79f0; // AudioEngine::~AudioEngine()
const VA_AUDIO_ENGINE_SET_VOLUME: usize = 0x1_401f_bdf0; // AudioEngine::setVolume(float)
const VA_AUDIO_ENGINE_PARSE_GUIDS: usize = 0x1_401f_a8c0; // AudioEngine::parseGUIDs(const wstring&)
const VA_AUDIO_ENGINE_ADD_CACHE: usize = 0x1_401f_8730; // AudioEngine::addCache(const wstring&)
const VA_AUDIO_ENGINE_LISTENER_DISTANCE: usize = 0x1_401f_a610; // float AudioEngine::listenerDistance(const vec3f&)
const VA_AUDIO_ENGINE_SET_LISTENER: usize = 0x1_401f_bb30; // AudioEngine::setListener(const mat44f&, const vec3f&)
const VA_AUDIO_ENGINE_UPDATE: usize = 0x1_401f_c080; // AudioEngine::update(float)
const VA_CAR_AUDIO_CTOR: usize = 0x1_4006_2830; // CarAudioFMOD::CarAudioFMOD(CarAvatar*)
const VA_CAR_AUDIO_DELETING_DTOR: usize = 0x1_4006_5820; // CarAudioFMOD::`scalar deleting destructor'(unsigned int)
const VA_CAR_AUDIO_RENDER_AUDIO: usize = 0x1_4006_69b0; // CarAudioFMOD::renderAudio(float)
const VA_CAR_AUDIO_ON_CAR_HIT: usize = 0x1_4006_6480; // CarAudioFMOD::onCarHit(float, const vec3f&, const vec3f&, float, float, unsigned long)
const VA_BACKFIRE_PARAMS_CTOR: usize = 0x1_400c_ccb0; // BackfireParams::BackfireParams(CarAvatar*)
const VA_BACKFIRE_CHECK: usize = 0x1_400d_26b0; // bool BackfireParams::checkBackfire(float)
const VA_EVENT_TRIGGER_UPDATE: usize = 0x1_400d_aef0; // EventTriggerOnChange<int>::update()
const VA_TRACK_AUDIO_CTOR: usize = 0x1_401c_24c0; // TrackAudio::TrackAudio(TrackAvatar*, Sim*)
const VA_TRACK_AUDIO_DELETING_DTOR: usize = 0x1_401c_3bd0; // TrackAudio::`scalar deleting destructor'(unsigned int)
const VA_TRACK_AUDIO_RENDER: usize = 0x1_401c_42a0; // TrackAudio::render(float)
const VA_PRESET_NAMES_INIT: usize = 0x1_4001_0980; // dynamic initialiser of AudioReverb's presetNames
const VA_ATEXIT: usize = 0x1_4039_9754; // atexit (the program's own, statically linked)
const VA_NODE_VTABLE: usize = 0x1_404e_b558; // Node
const VA_MESH_VTABLE: usize = 0x1_404e_da28; // Mesh
const VA_CAMERA_MANAGER_VTABLE: usize = 0x1_404a_6398; // ACCameraManager
const VA_INIREADERDOCUMENTS_INITIALIZED: usize = 0x1_4155_a588; // static bool INIReaderDocuments::initialized

unsafe fn wr<T>(base: *mut u8, offset: usize, value: T) {
    std::ptr::write_unaligned(base.add(offset) as *mut T, value);
}

unsafe fn rd<T: Copy>(base: *const u8, offset: usize) -> T {
    std::ptr::read_unaligned(base.add(offset) as *const T)
}

/// Writes an MSVC `std::wstring` (0x20 bytes: buffer or pointer, size, capacity).
unsafe fn write_wstring(acs: &Acs, at: *mut u8, text: &str) {
    let units: Vec<u16> = text.encode_utf16().collect();
    if units.len() < 8 {
        std::ptr::write_bytes(at, 0, 0x10);
        std::ptr::copy_nonoverlapping(units.as_ptr(), at as *mut u16, units.len());
        wr(at, 0x10, units.len());
        wr(at, 0x18, 7usize);
    } else {
        let buffer = acs.alloc((units.len() + 1) * 2);
        std::ptr::copy_nonoverlapping(units.as_ptr(), buffer as *mut u16, units.len());
        wr(at, 0, buffer);
        wr(at, 0x10, units.len());
        wr(at, 0x18, units.len());
    }
}

/// A `std::wstring` of its own, for a `const std::wstring&` argument.
unsafe fn wstring(acs: &Acs, text: &str) -> *mut u8 {
    let s = acs.alloc(0x20);
    write_wstring(acs, s, text);
    s
}

/// Writes an MSVC `std::vector` (begin, end, end of storage) holding `count` elements at `data`.
unsafe fn write_vector(at: *mut u8, data: *mut u8, bytes: usize) {
    wr(at, 0, data);
    wr(at, 8, data.add(bytes));
    wr(at, 0x10, data.add(bytes));
}

pub struct Harness<'a> {
    acs: &'a Acs,
    engine: *mut u8,
    sim: *mut u8,
    car: *mut u8,
    physics_car: *mut u8,
    car_audio: *mut u8,
    backfire: *mut u8,
    track_audio: *mut u8,
}

impl<'a> Harness<'a> {
    /// Builds the game's sound objects for a drive. The working directory must be the oracle's
    /// small game folder.
    pub unsafe fn build(acs: &'a Acs, ac: &Path, drive: &Drive, scene: Option<&Scene>, cache_paths: &[String], master: f32) -> Harness<'a> {
        // Documents files are read from the working directory, not from the user's Documents
        acs.set_global::<u8>(VA_INIREADERDOCUMENTS_INITIALIZED, 1);
        // the program's own `atexit` list was never set up (its start-up code does not run
        // here): the initialiser below must not register its destructor. xor eax,eax ; ret
        acs.patch(acs.va(VA_ATEXIT), &[0x31, 0xc0, 0xc3]);
        let init: extern "C" fn() = std::mem::transmute(acs.va(VA_PRESET_NAMES_INIT));
        init();

        let engine = acs.alloc(0x78);
        let ctor: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_AUDIO_ENGINE_CTOR));
        ctor(engine);
        // Game::Game: the master level of audio.ini
        let set_volume: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_AUDIO_ENGINE_SET_VOLUME));
        set_volume(engine, master);

        let game = acs.alloc(0x400);
        wr(game, 0x148, engine);
        let sim = acs.alloc(0x800);
        wr(sim, 0x8, game);

        // the camera manager: found by the sound through a dynamic cast, so it carries the
        // game's own vtable; the sound reads its mode and the two camera objects
        let cm = acs.alloc(0x400);
        wr(cm, 0, acs.va(VA_CAMERA_MANAGER_VTABLE));
        let view = drive.camera.view();
        wr(cm, 0x120, view.mode);
        let camera_car = acs.alloc(0x100);
        wr(camera_car, 0x68, view.car_camera_index);
        wr(cm, 0xb8, camera_car);
        let camera_drivable = acs.alloc(0x200);
        wr(camera_drivable, 0x58, view.drivable_mode);
        wr(camera_drivable, 0x78, sim);
        let positions = acs.alloc(0x38);
        wr(positions, 0x34, view.bumper_external_sound as u8);
        wr(positions, 0x35, view.bonnet_external_sound as u8);
        write_vector(camera_drivable.add(0x88), positions, 0x38);
        wr(cm, 0xc0, camera_drivable);
        let objects = acs.alloc(8);
        wr(objects, 0, cm);
        write_vector(sim.add(0x40), objects, 8);
        wr::<u32>(sim, 0x220, 0);

        // the track: its nodes with the game's own Node / Mesh vtables, then TrackAudio
        let mut track_audio = std::ptr::null_mut();
        if let (Some(scene), Some(relative)) = (scene, drive.track_data_relative()) {
            let mut nodes: Vec<*mut u8> = Vec::with_capacity(scene.nodes.len());
            for node in &scene.nodes {
                let p = acs.alloc(if node.renderable { 0x168 } else { 0xe0 });
                wr(p, 0, acs.va(if node.renderable { VA_MESH_VTABLE } else { VA_NODE_VTABLE }));
                std::ptr::copy_nonoverlapping(node.matrix.as_ptr(), p.add(0x08) as *mut f32, 16);
                std::ptr::copy_nonoverlapping(node.matrix.as_ptr(), p.add(0x48) as *mut f32, 16);
                write_wstring(acs, p.add(0xb8), &node.name);
                wr::<u8>(p, 0xd8, 1);
                if let Some(mesh) = &node.mesh {
                    let vertices = acs.alloc(mesh.positions.len().max(1) * 0x2c);
                    for (i, v) in mesh.positions.iter().enumerate() {
                        std::ptr::copy_nonoverlapping(v.as_ptr(), vertices.add(i * 0x2c) as *mut f32, 3);
                    }
                    write_vector(p.add(0x108), vertices, mesh.positions.len() * 0x2c);
                    let indices = acs.alloc(mesh.indices.len().max(1) * 2);
                    std::ptr::copy_nonoverlapping(mesh.indices.as_ptr(), indices as *mut u16, mesh.indices.len());
                    write_vector(p.add(0x120), indices, mesh.indices.len() * 2);
                }
                nodes.push(p);
            }
            for (i, node) in scene.nodes.iter().enumerate() {
                if let Some(parent) = node.parent {
                    wr(nodes[i], 0xa8, nodes[parent]);
                }
                if !node.children.is_empty() {
                    let list = acs.alloc(node.children.len() * 8);
                    for (k, &child) in node.children.iter().enumerate() {
                        wr(list, k * 8, nodes[child]);
                    }
                    write_vector(nodes[i].add(0x90), list, node.children.len() * 8);
                }
            }
            wr(sim, 0x128, nodes[0]);
            let track = acs.alloc(0x400);
            wr(track, 0x8, game);
            wr(track, 0xe8, sim);
            write_wstring(acs, track.add(0x200), &relative);
            track_audio = acs.alloc(0xb0);
            let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_TRACK_AUDIO_CTOR));
            ctor(track_audio, track, sim);
        }
        // Sim::loadTrack: the pool of surface sounds
        let add_cache: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(acs.va(VA_AUDIO_ENGINE_ADD_CACHE));
        for path in cache_paths {
            add_cache(engine, wstring(acs, path));
        }

        // the car: a CarAvatar laid out by hand
        let car = acs.alloc(0x12a8);
        wr(car, 0x8, game);
        wr(car, 0x130, sim);
        write_wstring(acs, car.add(0x138), &drive.car);
        write_wstring(acs, car.add(0x158), "");
        let car_node = acs.alloc(0xe0);
        wr::<u8>(car_node, 0xd8, 1);
        wr(car, 0x210, car_node);
        wr::<i32>(car, 0x1158, 0);
        let physics_car = acs.alloc(0x1000);
        wr(car, 0x1168, physics_car);
        // far from the pit box
        wr::<[f32; 3]>(car, 0x10a8, [0.0, -1000.0, 0.0]);
        if let Some(first) = drive.frames.first() {
            std::ptr::copy_nonoverlapping(first.state.as_ptr(), car.add(0x268), first.state.len());
        }
        let backfire = acs.alloc(0x28);
        let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_BACKFIRE_PARAMS_CTOR));
        ctor(backfire, car);
        wr(car, 0x1068, backfire);
        let cars = acs.alloc(8);
        wr(cars, 0, car);
        write_vector(sim.add(0x208), cars, 8);

        // CarAvatar::initCommonPostPhysics: the car's own GUIDs, then the sound object
        let mut car_audio = std::ptr::null_mut();
        if ac.join(rustyac_audio::car::CarAudio::bank_path(&drive.car)).is_file() {
            let guids = format!("content/cars/{}/sfx/GUIDs.txt", drive.car);
            if Path::new(&guids).is_file() {
                let parse: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(acs.va(VA_AUDIO_ENGINE_PARSE_GUIDS));
                parse(engine, wstring(acs, &guids));
            }
            car_audio = acs.alloc(0x4b8);
            let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_CAR_AUDIO_CTOR));
            ctor(car_audio, car);
            wr(car, 0xe18, car_audio);
        }
        // the gear trigger watches physicsState.gear
        wr(car, 0x78, car.add(0x4d4));
        Harness { acs, engine, sim, car, physics_car, car_audio, backfire, track_audio }
    }

    /// One frame, in the order of `rustyac_audio::sim::AudioWorld::frame`.
    pub unsafe fn frame(&mut self, frame: &crate::drive::Frame) {
        let acs = self.acs;
        let car = self.car;
        // the main thread's copy of the physics state
        std::ptr::copy_nonoverlapping(frame.state.as_ptr(), car.add(0x268), frame.state.len());
        wr::<u8>(self.physics_car, 0xa08, frame.car.tc_in_action as u8);
        // the drawn body's matrix: its position is all the sound reads
        wr::<[f32; 3]>(car, 0x254, frame.car.body_position);

        // Sim::stepPhysicsEvent
        if !self.car_audio.is_null() {
            let on_car_hit: extern "C" fn(*mut u8, f32, *const f32, *const f32, f32, f32, u32) = std::mem::transmute(acs.va(VA_CAR_AUDIO_ON_CAR_HIT));
            for raw in &frame.events {
                let p = raw.as_ptr();
                if rd::<i32>(p, 0) != 0 {
                    continue;
                }
                if rd::<f32>(p, 4) as i32 != 0 {
                    continue;
                }
                on_car_hit(self.car_audio, rd(p, 0x08), p.add(0x14).cast(), p.add(0x20).cast(), rd(p, 0x0c), rd(p, 0x10), rd(p, 0x40));
            }
        }
        // CarAvatar::update: the backfire test, then the event triggers
        let engine_life: f32 = rd(car, 0xa10);
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(0.0 >= engine_life) {
            let check: extern "C" fn(*mut u8, f32) -> u8 = std::mem::transmute(acs.va(VA_BACKFIRE_CHECK));
            if check(self.backfire, DT) != 0 {
                let arg = 0u8;
                self.fire(car.add(0xd0), &arg as *const u8);
            }
        }
        let trigger: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_EVENT_TRIGGER_UPDATE));
        trigger(car.add(0x58));
        // Sim::update: the cars ranked by distance to the listener (one car: rank 0)
        if !self.car_audio.is_null() {
            let distance: extern "C" fn(*mut u8, *const f32) -> f32 = std::mem::transmute(acs.va(VA_AUDIO_ENGINE_LISTENER_DISTANCE));
            let d = distance(self.engine, car.add(0x254).cast());
            wr::<i32>(self.car_audio, 0x4a8, 0);
            wr::<f32>(self.car_audio, 0x4ac, d - 0.0);
        } else {
            let distance: extern "C" fn(*mut u8, *const f32) -> f32 = std::mem::transmute(acs.va(VA_AUDIO_ENGINE_LISTENER_DISTANCE));
            distance(self.engine, car.add(0x254).cast());
        }
        // the camera
        let set_listener: extern "C" fn(*mut u8, *const f32, *const f32) = std::mem::transmute(acs.va(VA_AUDIO_ENGINE_SET_LISTENER));
        set_listener(self.engine, frame.listener.0.as_ptr(), frame.listener.1.as_ptr());
        // Game::render, Game::renderAudio, AudioEngine::update
        if !self.track_audio.is_null() {
            let render: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_TRACK_AUDIO_RENDER));
            render(self.track_audio, DT);
        }
        if !self.car_audio.is_null() {
            let render_audio: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_CAR_AUDIO_RENDER_AUDIO));
            render_audio(self.car_audio, DT);
        }
        let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_AUDIO_ENGINE_UPDATE));
        update(self.engine, DT);
    }

    /// Calls every handler of an `Event<T>` (a vector of 0x28-byte pairs of owner and
    /// `std::function`), as the game's own loops do.
    unsafe fn fire(&self, event: *mut u8, arg: *const u8) {
        let mut at: *mut u8 = rd(event, 0);
        let end: *mut u8 = rd(event, 8);
        while at != end {
            let implementation: *mut u8 = rd(at, 0x20);
            let vtable: *const usize = rd(implementation, 0);
            let call: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(*vtable.add(2));
            call(implementation, arg);
            at = at.add(0x28);
        }
    }

    /// The destructors, in the order the port tears its world down.
    pub unsafe fn teardown(self) {
        let acs = self.acs;
        if !self.car_audio.is_null() {
            let dtor: extern "C" fn(*mut u8, u32) -> *mut u8 = std::mem::transmute(acs.va(VA_CAR_AUDIO_DELETING_DTOR));
            dtor(self.car_audio, 0);
        }
        if !self.track_audio.is_null() {
            let dtor: extern "C" fn(*mut u8, u32) -> *mut u8 = std::mem::transmute(acs.va(VA_TRACK_AUDIO_DELETING_DTOR));
            dtor(self.track_audio, 0);
        }
        let dtor: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_AUDIO_ENGINE_DTOR));
        dtor(self.engine);
        let _ = self.sim;
    }
}

/// Runs a drive through the game's own sound code.
pub fn run(acs: &Acs, ac: &Path, drive: &Drive, scene: Option<&Scene>, cache_paths: &[String], master: f32) {
    unsafe {
        log::mark("setup");
        let mut harness = Harness::build(acs, ac, drive, scene, cache_paths, master);
        for (i, frame) in drive.frames.iter().enumerate() {
            log::mark(&format!("frame {i}"));
            harness.frame(frame);
        }
        log::mark("teardown");
        harness.teardown();
    }
}
