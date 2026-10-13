// SPDX-License-Identifier: GPL-3.0-or-later

//! The picture of the game: AC's own renderer (the port in `rustyac-render`), or the old debug
//! view (`--debug-view`), behind one set of calls.

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dxgi::IDXGISwapChain1;

use super::ac::AcRenderer;
use super::hud::HudInfo;
use super::scene::{CameraFrame, CarShape, DrivingCamera};
use super::DebugRenderer;
use crate::view::CarView;

pub enum Picture {
    Debug(Box<DebugRenderer>),
    Ac(Box<AcRenderer>),
}

impl Picture {
    /// Task 28: the referee's race start, `ms` from now, for the start lights.
    pub fn set_race_start_in(&mut self, ms: Option<f64>) {
        if let Picture::Ac(renderer) = self {
            renderer.set_race_start_in(ms);
        }
    }

    /// Who draws, in words.
    pub fn describe(&self) -> String {
        match self {
            Picture::Debug(r) => format!("the debug view, {} samples per pixel, {}{}", r.samples(), r.adapter, if r.software { " (software rasteriser)" } else { "" }),
            Picture::Ac(r) => format!("AC's renderer, {}{}", r.adapter, if r.is_warp() { " (WARP, the software rasteriser)" } else { "" }),
        }
    }

    pub fn size(&self) -> (u32, u32) {
        match self {
            Picture::Debug(r) => r.size(),
            Picture::Ac(r) => r.size(),
        }
    }

    /// Whether the car's backfire test of the last frame fired; `None`: nothing ran one.
    pub fn backfire_triggered(&self) -> Option<bool> {
        match self {
            Picture::Debug(_) => None,
            Picture::Ac(renderer) => renderer.backfire_triggered(),
        }
    }

    /// One frame of the one car (a session without other cars, or a caller that only knows
    /// the player's).
    pub fn draw(&mut self, view: &CarView, shape: &CarShape, driving: &DrivingCamera, frame: &CameraFrame, info: &HudInfo, dt: f32) {
        self.draw_world(std::slice::from_ref(view), 0, shape, driving, frame, info, dt);
    }

    /// One frame of every car of the session (`world`: the player's first) with the camera on
    /// the car `focus`. The debug view draws the player's car and the others as boxes.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_world(&mut self, world: &[CarView], focus: usize, shape: &CarShape, driving: &DrivingCamera, frame: &CameraFrame, info: &HudInfo, dt: f32) {
        match self {
            Picture::Debug(r) => r.draw(&world[0], shape, frame, info),
            Picture::Ac(r) => r.draw(world, focus, driving, frame, info, dt),
        }
    }

    /// AC's GUI and HUD, when they are drawn.
    pub fn gui(&mut self) -> Option<&mut super::gui::GameGui> {
        match self {
            Picture::Ac(r) => r.gui.as_deref_mut(),
            Picture::Debug(_) => None,
        }
    }

    /// Task 28c: the states a replay shows (`None`: the cars are live).
    pub fn set_replay_states(&mut self, states: Option<Vec<rustyac_physics::car_physics_state::CarPhysicsState>>) {
        if let Picture::Ac(r) = self {
            r.replay_states = states;
        }
    }

    /// The session's ghost car: its recorder, which the physics thread fills.
    pub fn set_ghost(&mut self, rig: std::sync::Arc<std::sync::Mutex<crate::ghost::GhostRig>>) {
        if let Picture::Ac(r) = self {
            r.ghost_rig = Some(rig);
        }
    }

    /// The tyre changers' sound and camera link (AC's renderer with a pit crew only).
    pub fn crew_link(&mut self) -> Option<&mut super::ac::CrewLink> {
        match self {
            Picture::Debug(_) => None,
            Picture::Ac(r) => Some(&mut r.crew),
        }
    }

    /// By wheel: the gun the mechanic there is on in this frame (`Some(true)`: screwing,
    /// `Some(false)`: unscrewing).
    pub fn crew_guns_wanted(&self) -> [Option<bool>; 4] {
        match self {
            Picture::Debug(_) => [None; 4],
            Picture::Ac(r) => r.crew_guns_wanted(),
        }
    }

    /// Whether each car's backfire test of the last frame fired, in the order of the cars.
    pub fn backfires_triggered(&self) -> Vec<Option<bool>> {
        match self {
            Picture::Debug(_) => Vec::new(),
            Picture::Ac(renderer) => renderer.backfires_triggered(),
        }
    }

    /// F11: the virtual mirror on or off (AC's renderer with mirrors on only).
    pub fn toggle_virtual_mirror(&mut self) -> Option<bool> {
        match self {
            Picture::Debug(_) => None,
            Picture::Ac(r) => r.toggle_virtual_mirror(),
        }
    }

    /// Ctrl+I: the ideal line (AC's renderer only).
    pub fn toggle_ideal_line(&mut self) -> Option<bool> {
        match self {
            Picture::Debug(_) => None,
            Picture::Ac(r) => r.toggle_ideal_line(),
        }
    }

    /// What the motion blur remembers of the last frame, to draw one frame twice.
    pub fn blur_history(&self) -> Option<(rustyac_physics::vecmath::Mat44f, f32)> {
        match self {
            Picture::Ac(r) => Some(r.blur_history()),
            Picture::Debug(_) => None,
        }
    }

    pub fn set_blur_history(&mut self, history: Option<(rustyac_physics::vecmath::Mat44f, f32)>) {
        if let (Picture::Ac(r), Some(history)) = (self, history) {
            r.set_blur_history(history);
        }
    }

    /// The picture `ScreenCapturer` took in the last frame, if it took one: width, height, RGBA.
    pub fn take_pre_gui_shot(&mut self) -> Option<super::ac::ScreenShot> {
        match self {
            Picture::Ac(r) => r.pre_gui_shot.take(),
            Picture::Debug(_) => None,
        }
    }

    pub fn set_virtual_mirror(&mut self, active: bool) {
        if let Picture::Ac(r) = self {
            r.set_virtual_mirror(active);
        }
    }

    pub fn read_pixels(&mut self) -> Result<Vec<u8>, String> {
        match self {
            Picture::Debug(r) => r.read_pixels(),
            Picture::Ac(r) => r.read_pixels(),
        }
    }

    pub fn finish(&self) {
        match self {
            Picture::Debug(r) => r.finish(),
            Picture::Ac(r) => r.finish(),
        }
    }

    pub fn swap_chain(&self, window: HWND) -> Result<IDXGISwapChain1, String> {
        match self {
            Picture::Debug(r) => r.swap_chain(window),
            Picture::Ac(r) => r.swap_chain(window),
        }
    }

    pub fn resize_swap_chain(&mut self, chain: &IDXGISwapChain1, width: u32, height: u32) -> Result<(), String> {
        match self {
            Picture::Debug(r) => r.resize_swap_chain(chain, width, height),
            Picture::Ac(r) => r.resize_swap_chain(chain, width, height),
        }
    }

    pub fn present(&self, chain: &IDXGISwapChain1, vsync: bool) -> Result<bool, String> {
        match self {
            Picture::Debug(r) => r.present(chain, vsync),
            Picture::Ac(r) => r.present(chain, vsync),
        }
    }

    /// What the last frame drew, where that is counted.
    pub fn frame_stats(&self) -> Option<String> {
        match self {
            Picture::Debug(_) => None,
            Picture::Ac(r) => Some(format!("{} draw calls, {} triangles in the last frame", r.draw_calls, r.triangles)),
        }
    }
}
