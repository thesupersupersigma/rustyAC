// SPDX-License-Identifier: GPL-3.0-or-later

//! The pit crew's wheel guns in the audio oracle (`--crew <frame>:<seconds>`): from that frame
//! of the drive the four tyre changers work through a tyre change of so many seconds.
//!
//! The game's side: a `PitCrew` laid out by hand whose four `CrewMember`s have the game's own
//! `AudioEvent`s (made by `AudioEvent::AudioEvent` on the oracle's engine) and stand-in nodes;
//! the game's own `PitCrew::onPitStopBegin` 0x1400a33e0 and `updateCurrentPosition`
//! 0x1400a4500 run on it, so every FMOD call of the guns is the game's. The port's side:
//! `rustyac_render::tyre_crew::PitCrew` on nodes without a model, its sound through
//! `rustyac_audio::pit_crew::CrewGuns`. Both read the timeline's numbers from the game's
//! `content/objects3D/pitcrewtyre.ini` and draw the mechanics' times from `rand()` seeded 28.

use std::path::Path;

use rustyac_audio::engine::AudioEngine;
use rustyac_audio::pit_crew::CrewGuns;
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::Mat44f;
use rustyac_render::animator::{Animation, AnimationPlayer};
use rustyac_render::scene::Scene;
use rustyac_render::tyre_crew::{CrewHost, CrewInputs, CrewNodes, CrewPitStop, CrewStart, CrewTimeline, PitCrew};

use crate::acs::Acs;

const VA_AUDIO_EVENT_CTOR: usize = 0x1_401f_7630; // AudioEvent::AudioEvent(AudioEngine*, const std::wstring&, ReverbResponse)
const VA_PIT_CREW_BEGIN: usize = 0x1_400a_33e0; // PitCrew::onPitStopBegin(const PitStopStart&)
const VA_PIT_CREW_UPDATE_CURRENT_POSITION: usize = 0x1_400a_4500; // PitCrew::updateCurrentPosition(float)
const VA_NODE_VTABLE: usize = 0x1_404e_b558; // Node

/// `--crew <frame>:<seconds>`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CrewSpec {
    pub at: usize,
    pub seconds: f32,
}

impl CrewSpec {
    pub fn parse(text: &str) -> Result<CrewSpec, String> {
        let (at, seconds) = text.split_once(':').ok_or("--crew <frame>:<seconds>")?;
        Ok(CrewSpec { at: at.parse().map_err(|_| "--crew <frame>:<seconds>")?, seconds: seconds.parse().map_err(|_| "--crew <frame>:<seconds>")? })
    }
}

/// The seed of the mechanics' times on both sides.
const SEED: u32 = 28;

fn timeline(ac: &Path) -> CrewTimeline {
    let ini = IniReader::load(&ac.join("content/objects3D/pitcrewtyre.ini")).expect("pitcrewtyre.ini");
    CrewTimeline::read(&ini, 10.0)
}

unsafe fn wr<T>(base: *mut u8, offset: usize, value: T) {
    base.add(offset).cast::<T>().write_unaligned(value);
}

/// The game's side.
pub struct GameCrew {
    crew: *mut u8,
    pit_stop: *mut u8,
    spec: CrewSpec,
    on: bool,
}

impl GameCrew {
    /// `engine`, `sim`, `car`: the harness's `AudioEngine`, `Sim` and first `CarAvatar`;
    /// `wstring`: a `std::wstring` of the game's own allocator.
    pub unsafe fn build(acs: &Acs, ac: &Path, engine: *mut u8, sim: *mut u8, car: *mut u8, spec: CrewSpec, wstring: &dyn Fn(&str) -> *mut u8) -> GameCrew {
        let t = timeline(ac);
        let node = || {
            let n = acs.alloc(0xe0);
            wr(n, 0, acs.va(VA_NODE_VTABLE));
            wr(n, 8, Mat44f::IDENTITY.m);
            wr(n, 0x48, Mat44f::IDENTITY.m);
            wr(n, 0xd8, 1u8);
            n
        };
        let members = acs.alloc(0xc8 * 4);
        let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8, i32) -> *mut u8 = std::mem::transmute(acs.va(VA_AUDIO_EVENT_CTOR));
        for k in 0..4 {
            let m = members.add(0xc8 * k);
            wr(m, 0x18, acs.alloc(0x28)); // an AnimationPlayer with no sets
            wr(m, 0x20, node()); // model
            wr(m, 0x28, node()); // oldTyreNull
            wr(m, 0x30, node()); // newTyreNull
            wr(m, 0x38, node()); // newTyreModel
            wr(m, 0x40, node()); // defaultPosition
            let screw = acs.alloc(0x48);
            ctor(screw, engine, wstring("event:/common/screw"), 0);
            let unscrew = acs.alloc(0x48);
            ctor(unscrew, engine, wstring("event:/common/unscrew"), 0);
            wr(m, 0x78, screw);
            wr(m, 0x80, unscrew);
            wr(m, 0xa0, t.loop_speed_mult);
            wr(m, 0xa4, t.percentage_begin_un_screw);
            wr(m, 0xa8, t.percentage_un_screw);
            wr(m, 0xac, t.percentage_end_un_screw);
            wr(m, 0xb0, t.percentage_begin_screw);
            wr(m, 0xb4, t.percentage_screw);
            wr(m, 0xb8, t.percentage_end_screw);
            wr(m, 0xbc, t.time_to_position_default);
            wr(m, 0xc0, t.time_to_change_tyre_default);
            wr(m, 0xc4, t.time_to_finish_default);
        }
        let pit_stop = acs.alloc(0x240);
        wr(pit_stop, 0x120, 3i32);
        let crew = acs.alloc(0xa8);
        wr(crew, 0x68, members);
        wr(crew, 0x70, members.add(0xc8 * 4));
        wr(crew, 0x78, members.add(0xc8 * 4));
        wr(crew, 0x80, pit_stop);
        // a Sim of the crew's own: the camera manager (its mode is read when the stop begins),
        // no VR, the focused car is the crew's
        let _ = sim;
        let crew_sim = acs.alloc(0x800);
        wr(crew_sim, 0x188, acs.alloc(0x400));
        wr(crew, 0x88, crew_sim);
        wr(crew, 0x90, car);
        GameCrew { crew, pit_stop, spec, on: false }
    }

    /// The start of a frame: the stop begins at its frame; then the frame's work.
    pub unsafe fn frame(&mut self, acs: &Acs, index: usize) {
        if index < self.spec.at {
            return;
        }
        if !self.on {
            self.on = true;
            let srand: extern "C" fn(u32) = std::mem::transmute(acs.crt_function(c"srand"));
            srand(SEED);
            wr(self.pit_stop, 0xa4, self.spec.seconds);
            wr(self.pit_stop, 0xac, self.spec.seconds);
            wr(self.pit_stop, 0xbc, self.spec.seconds);
            let mut start = [0u8; 12];
            start[0..4].copy_from_slice(&self.spec.seconds.to_le_bytes());
            start[4..8].copy_from_slice(&self.spec.seconds.to_le_bytes());
            let begin: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(acs.va(VA_PIT_CREW_BEGIN));
            begin(self.crew, start.as_ptr());
        }
        let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_PIT_CREW_UPDATE_CURRENT_POSITION));
        update(self.crew, crate::drive::DT);
    }
}

/// The port's side: the hooks of the session.
pub struct PortCrew<'a> {
    spec: CrewSpec,
    timeline: CrewTimeline,
    scene: Scene,
    crew: Option<PitCrew>,
    guns: Option<CrewGuns>,
    rand: rustyac_physics::session::MsvcRand,
    /// the raw `CarPhysicsState` of every frame
    states: Vec<&'a [u8]>,
    on: bool,
}

impl<'a> PortCrew<'a> {
    pub fn new(ac: &Path, spec: CrewSpec, states: Vec<&'a [u8]>) -> PortCrew<'a> {
        PortCrew { spec, timeline: timeline(ac), scene: Scene::new(), crew: None, guns: None, rand: rustyac_physics::session::MsvcRand(SEED), states, on: false }
    }
}

struct Host<'a> {
    engine: &'a AudioEngine,
    guns: &'a CrewGuns,
}

impl CrewHost for Host<'_> {
    fn camera_mode(&mut self) -> i32 {
        0
    }
    fn set_camera_mode(&mut self, _mode: i32, _a: bool, _b: bool) {}
    fn force_theta_phi(&mut self, _theta: f32, _phi: f32, _radius: f32) {}
    fn lock_camera(&mut self, _locked: bool) {}
    fn fade_in(&mut self, _fade_total_time: f32) {}
    fn event_is_playing(&mut self, wheel: usize, screw: bool) -> bool {
        self.guns.is_playing(self.engine, wheel, screw)
    }
    fn event_start(&mut self, wheel: usize, screw: bool, matrix: &Mat44f) {
        let mut m = [0.0f32; 16];
        for (r, row) in matrix.m.iter().enumerate() {
            m[r * 4..r * 4 + 4].copy_from_slice(row);
        }
        self.guns.start(self.engine, wheel, screw, &m);
    }
    fn event_stop(&mut self, wheel: usize, screw: bool) {
        self.guns.stop(self.engine, wheel, screw);
    }
}

impl rustyac_audio::golden::SessionHooks for PortCrew<'_> {
    fn setup(&mut self, engine: &mut AudioEngine) {
        self.guns = Some(CrewGuns::new(engine));
        let mut nodes = Vec::new();
        for _ in 0..4 {
            let model = self.scene.node("model");
            let player = AnimationPlayer::new(&Animation::default(), &self.scene, model);
            nodes.push(CrewNodes { player, model, old_tyre_null: self.scene.node("old"), new_tyre_null: self.scene.node("new"), new_tyre_model: self.scene.node("tyre"), default_position: self.scene.node("position"), car_wheels: Vec::new() });
        }
        self.crew = Some(PitCrew::on_nodes(&mut self.scene, nodes, &self.timeline));
    }

    fn frame(&mut self, engine: &mut AudioEngine, index: usize) {
        if index < self.spec.at {
            return;
        }
        let (Some(crew), Some(guns)) = (&mut self.crew, &self.guns) else { return };
        // (the oracle runs the crew at the start of a frame, before the car takes the frame's
        // state: both sides read the car as the frame before left it)
        let state = self.states[index.saturating_sub(1)];
        let f = |at: usize| f32::from_le_bytes(state[at..at + 4].try_into().unwrap());
        let mut tyre_matrix = [Mat44f::IDENTITY; 4];
        let mut tyre_contact_point = [[0.0f32; 3]; 4];
        for k in 0..4 {
            for i in 0..16 {
                tyre_matrix[k].m[i / 4][i % 4] = f(0x144 + 0x40 * k + 4 * i);
            }
            for i in 0..3 {
                tyre_contact_point[k][i] = f(0x67c + 12 * k + 4 * i);
            }
        }
        let t = self.spec.seconds;
        let inputs = CrewInputs { replay: false, pause_menu: false, session_type: 1, pit: CrewPitStop { state: 3, pit_timer: t, max_pit_timer: t, tyre_time: t }, tyre_matrix, tyre_contact_point, focused: true };
        let mut host = Host { engine, guns };
        if !self.on {
            self.on = true;
            let rand = &mut self.rand;
            crew.on_pit_stop_begin(&mut self.scene, &mut host, &mut || rand.next(), &inputs, &CrewStart { tyre_change_time: t, total_time: t, force_change_camera: false });
        }
        crew.pit_crew_update(&mut self.scene, &mut host, crate::drive::DT, &inputs);
    }
}
