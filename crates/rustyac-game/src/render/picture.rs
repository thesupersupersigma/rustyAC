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

    pub fn draw(&mut self, view: &CarView, shape: &CarShape, driving: &DrivingCamera, frame: &CameraFrame, info: &HudInfo, dt: f32) {
        match self {
            Picture::Debug(r) => r.draw(view, shape, frame, info),
            Picture::Ac(r) => r.draw(view, driving, frame, info, dt),
        }
    }

    /// F11: the virtual mirror on or off (AC's renderer with mirrors on only).
    pub fn toggle_virtual_mirror(&mut self) -> Option<bool> {
        match self {
            Picture::Debug(_) => None,
            Picture::Ac(r) => r.toggle_virtual_mirror(),
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
