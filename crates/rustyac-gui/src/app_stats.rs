// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The statistics apps: `ksgui::FormRenderStats` (0x1f0 bytes, "Render Stats": frames per
//! second, frame time, CPU, draw calls, triangles, physics, FMOD) and `FormGameStats` (0x1e8
//! bytes, the developers' "Graphics stats": the frame's update, scene, HUD and audio times).
//!
//! `FormRenderStats::FormRenderStats` 0x1402d69d0, `render` 0x1402d7540, fed by `Sim::update`'s
//! statistics block (0x14019f1e0..0x14019f660); `FormGameStats::FormGameStats` 0x1402e7a10,
//! `render` 0x1402e8140.

use std::any::Any;

use crate::apps::{App, AppAction};
use crate::control::Id;
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::Painter;
use crate::widgets2::Connected;

/// What the statistics apps read of the game (`Game::gameTime`, `Game::stats`,
/// `GraphicsManager::stats`, `PhysicsAvatar`, `AudioEngine::getPerformanceStats`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppStats {
    /// `GameTime::fps`, `deltaT` (s)
    pub fps: f32,
    pub delta_t: f32,
    /// `GraphicsManager::stats`: `dipCalls`, `triangles`, `sceneTriangles` (the frame's 3D part)
    pub dip_calls: i32,
    pub triangles: i32,
    pub scene_triangles: i32,
    /// `Game::stats` (ms): `cpuTime`, `updateTime`, `renderHUDTime`, `renderTime`,
    /// `renderAudioTime`
    pub cpu_time: f64,
    pub update_time: f64,
    pub render_hud_time: f64,
    pub render_time: f64,
    pub render_audio_time: f64,
    /// `PhysicsAvatar::occupancy` (percent of the 3 ms step, 0..300), `physicsLateLoops`
    pub occupancy: i32,
    pub physics_late_loops: u32,
    /// `AudioStats::updateCpuUsage` (none: no audio engine)
    pub fmod_update_usage: Option<f32>,
}

/// `GameTime` (`Game::gameTime`): the frame's time, smoothed, and the frames per second.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GameTime {
    pub delta_t: f32,
    pub smooth_delta_t: f32,
    pub fps: f32,
}

impl GameTime {
    /// `GameTime::update` 0x14044c250 after its clock: `d` the milliseconds since the last
    /// frame (no stabiliser, no frame cap).
    pub fn update(&mut self, d: f64) {
        let d = if d < 0.0 { 0.0 } else { d };
        let dt = (d * f64::from_bits(0x3f50_624d_d2f1_a9fc)) as f32;
        self.delta_t = if dt > f32::from_bits(0x3e4c_cccd) { f32::from_bits(0x3e4c_cccd) } else { dt };
        let smooth = self.delta_t * f32::from_bits(0x3c23_d70a) + self.smooth_delta_t * f32::from_bits(0x3f7d_70a4);
        self.smooth_delta_t = smooth;
        let smooth2 = smooth * f32::from_bits(0x3f66_6666) + self.delta_t * f32::from_bits(0x3dcc_cccd);
        self.smooth_delta_t = smooth2;
        self.fps = if smooth2 == 0.0 { 1.0 } else { 1.0 / smooth2 };
    }
}

/// `FormRenderStats`'s own members.
pub struct RenderStats {
    pub cpu_time: f32,
    pub simulation_occupancy: f32,
    pub render_audio_time: f32,
    pub physics_late: f32,
    pub fmod_update_usage: f32,
    pub tri_batch: i32,
    pub frame_time: f32,
    pub fps_graph: Id,
    pub smooth_fps: f32,
    /// FPS, FT, MAIN_T, DIP, TRI, B.SIZE, SCENE, PHY, PHY_LATE, FMOD USAGE
    pub labels: [Id; 10],
}

/// `FormGameStats`'s own members.
pub struct GameStats {
    pub update_time: f32,
    pub render_time: f32,
    pub hud_render_time: f32,
    pub physics_time: f32,
    pub cpu_time: f32,
    pub render_audio_time: f32,
    pub cpu_diff: f32,
    /// UPD, REND, HUD, PHYS, CPU, AUDIO, DIFF
    pub labels: [Id; 7],
}

/// The members `Sim::update` gives `FormRenderStats` (the values blended toward the frame's
/// by `dt`, the frame's seconds, clamped to 0..1).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SimStats {
    pub cpu_time: f32,
    pub simulation_occupancy: f32,
    pub render_audio_time: f32,
    pub physics_late: f32,
    pub fmod_update_usage: f32,
}

impl SimStats {
    /// `Sim::update`'s statistics block; true: "CPU OCCUPANCY > 99%" (a message every frame it
    /// holds).
    pub fn update(&mut self, dt: f32, s: &AppStats) -> bool {
        let k = if dt > 1.0 {
            1.0
        } else if dt >= 0.0 {
            dt
        } else {
            0.0
        };
        let occ = s.occupancy as f32;
        self.simulation_occupancy = (occ - self.simulation_occupancy) * k + self.simulation_occupancy;
        let d = (dt * 1000.0) as f64;
        let x = ((s.cpu_time / d) * 100.0) as f32;
        self.cpu_time = (x - self.cpu_time) * k + self.cpu_time;
        let warn = self.simulation_occupancy >= 99.0;
        self.render_audio_time = s.render_audio_time as f32;
        self.physics_late = s.physics_late_loops.min(10000) as f32;
        if let Some(usage) = s.fmod_update_usage {
            self.fmod_update_usage = usage;
        }
        warn
    }
}

impl Gui {
    /// `ksgui::FormRenderStats::FormRenderStats(GUI*, GameTime*)` 0x1402d69d0.
    pub fn new_render_stats_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "RSTATS", false);
        self.form_set_title(form, &translate("Render Stats"));
        self.form_data_mut(form).dev_app = false;
        let rows: [(&str, Connected, f32, &str, i32); 10] = [
            ("FPS", Connected::Float(0.0), 32.0, "", 3),
            ("FT", Connected::Float(0.0), 57.0, "ms", 1),
            ("MAIN_T", Connected::Float(0.0), 82.0, "%", 1),
            ("DIP", Connected::Int(0), 107.0, "", 3),
            ("TRI", Connected::Int(0), 132.0, "", 3),
            ("B.SIZE", Connected::Int(0), 157.0, "", 3),
            ("SCENE", Connected::Int(0), 182.0, "", 3),
            ("PHY", Connected::Float(0.0), 207.0, "%", 0),
            ("PHY_LATE", Connected::Float(0.0), 232.0, "", 0),
            ("FMOD USAGE", Connected::Float(0.0), 257.0, "%", 0),
        ];
        let mut labels = [0; 10];
        for (i, (name, value, y, suffix, precision)) in rows.into_iter().enumerate() {
            let l = self.new_connected_label(painter, name, value);
            self.set_position(l, 5.0, y);
            self.set_size(l, 50.0, 32.0);
            {
                let c = self.connected_label_mut(l);
                c.suffix = suffix.to_string();
                c.precision = precision;
            }
            self.control_add_control(form, l);
            labels[i] = l;
        }
        let fps_graph = self.new_graph(painter, "FPS_GRAPH");
        self.graph_mut(fps_graph).max_values_count = 50;
        self.set_position(fps_graph, 5.0, 282.0);
        self.set_size(fps_graph, 150.0, 50.0);
        self.graph_add_serie(fps_graph, [1.0, 0.0, 0.0]);
        self.graph_add_serie(fps_graph, [0.0, 0.0, 1.0]);
        self.graph_add_serie(fps_graph, [0.0, 1.0, 1.0]);
        {
            let g = self.graph_mut(fps_graph);
            g.max_y = 48.0;
            g.min_y = 0.0;
        }
        self.control_add_control(form, fps_graph);
        self.set_size(form, 160.0, 342.0);
        let m = self.controls[form].scale_mult;
        self.scale_by_mult(form, m);
        self.set_app(form, Box::new(RenderStats { cpu_time: 0.0, simulation_occupancy: 0.0, render_audio_time: 0.0, physics_late: 0.0, fmod_update_usage: 0.0, tri_batch: 0, frame_time: 0.0, fps_graph, smooth_fps: 0.0, labels }));
        form
    }

    /// `FormGameStats::FormGameStats(Game*)` 0x1402e7a10.
    pub fn new_game_stats_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "GAME_STATS", false);
        self.form_set_title(form, &translate("Graphics stats"));
        self.set_size(form, 100.0, 200.0);
        let mut labels = [0; 7];
        for (i, (name, y)) in [("UPD", 32.0), ("REND", 52.0), ("HUD", 72.0), ("PHYS", 92.0), ("CPU", 112.0), ("AUDIO", 132.0), ("DIFF", 152.0)].into_iter().enumerate() {
            let l = self.new_connected_label(painter, name, Connected::Float(0.0));
            self.set_position(l, 5.0, y);
            self.set_size(l, 50.0, 32.0);
            self.control_add_control(form, l);
            labels[i] = l;
        }
        self.set_app(form, Box::new(GameStats { update_time: 0.0, render_time: 0.0, hud_render_time: 0.0, physics_time: 0.0, cpu_time: 0.0, render_audio_time: 0.0, cpu_diff: 0.0, labels }));
        form
    }

    /// `Sim::update`'s statistics block on the "Render Stats" app (every frame, shown or not).
    pub fn stats_sim_update(&mut self, dt: f32) {
        let stats = self.apps.stats.clone();
        for id in 0..self.controls.len() {
            if let Some(app) = self.app_mut::<RenderStats>(id) {
                let mut s = SimStats { cpu_time: app.cpu_time, simulation_occupancy: app.simulation_occupancy, render_audio_time: app.render_audio_time, physics_late: app.physics_late, fmod_update_usage: app.fmod_update_usage };
                let warn = s.update(dt, &stats);
                (app.cpu_time, app.simulation_occupancy, app.render_audio_time, app.physics_late, app.fmod_update_usage) = (s.cpu_time, s.simulation_occupancy, s.render_audio_time, s.physics_late, s.fmod_update_usage);
                if warn {
                    self.apps.actions.push(AppAction::AddMessage { title: "WARNING".into(), text: "CPU OCCUPANCY > 99% ".into() });
                }
            }
        }
    }
}

impl App for RenderStats {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `FormRenderStats::render` 0x1402d7540.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        let s = gui.apps.stats.clone();
        self.tri_batch = if s.dip_calls == 0 { 0 } else { s.triangles.wrapping_div(s.dip_calls) };
        self.frame_time = s.delta_t * 1000.0;
        gui.graph_add_value(self.fps_graph, 0, s.delta_t * 1000.0);
        let max_y = gui.graph(self.fps_graph).max_y;
        gui.graph_add_value(self.fps_graph, 1, (self.simulation_occupancy * f32::from_bits(0x3c23_d70a)) * max_y);
        gui.graph_add_value(self.fps_graph, 2, self.render_audio_time);
        gui.graph_mut(self.fps_graph).max_values_count = 75;
        let fps = s.fps;
        let half = fps * 0.5;
        if (self.smooth_fps - fps).abs() > half {
            self.smooth_fps = fps;
        }
        self.smooth_fps = (fps - self.smooth_fps) * f32::from_bits(0x3c23_d70a) + self.smooth_fps;
        // the labels' connected values as the children read them while they draw
        let values = [
            Connected::Float(self.smooth_fps),
            Connected::Float(self.frame_time),
            Connected::Float(self.cpu_time),
            Connected::Int(s.dip_calls),
            Connected::Int(s.triangles),
            Connected::Int(self.tri_batch),
            Connected::Int(s.scene_triangles),
            Connected::Float(self.simulation_occupancy),
            Connected::Float(self.physics_late),
            Connected::Float(self.fmod_update_usage),
        ];
        for (l, v) in self.labels.iter().zip(values) {
            gui.connected_label_mut(*l).value = v;
        }
        gui.control_render(id, dt, painter);
    }
}

impl App for GameStats {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `FormGameStats::render` 0x1402e8140.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        let s = gui.apps.stats.clone();
        self.update_time = s.update_time as f32;
        self.render_time = s.render_time as f32;
        self.hud_render_time = s.render_hud_time as f32;
        self.cpu_time = s.cpu_time as f32;
        self.render_audio_time = s.render_audio_time as f32;
        self.cpu_diff = (s.delta_t * 1000.0) - self.cpu_time;
        let values = [self.update_time, self.render_time, self.hud_render_time, self.physics_time, self.cpu_time, self.render_audio_time, self.cpu_diff];
        for (l, v) in self.labels.iter().zip(values) {
            gui.connected_label_mut(*l).value = Connected::Float(v);
        }
        gui.control_render(id, dt, painter);
    }
}
