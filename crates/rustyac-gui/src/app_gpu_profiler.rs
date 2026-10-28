// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `GPUProfilerApp` (0x1c0), the developers' "GPU profiler": the renderer's timestamp queries
//! of the last measured frame, a row per pass, and the frame's time.
//!
//! `GPUProfilerApp::GPUProfilerApp(GraphicsManager*, GUI*)` 0x14031b320, `render` 0x14031b4d0.
//! The queries themselves are `rustyac_render::gpu_profiler::GpuProfiler`.

use std::any::Any;

use crate::apps::App;
use crate::control::Id;
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{Painter, ALIGN_LEFT};

/// One `GPUProfiler::QueryData` as `getResults` copies it: the pass, its time since the row
/// before (the first row: its own timestamp), its timestamp (ms).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuQueryRow {
    pub name: String,
    pub last_result: f64,
    pub last_ts: f64,
}

/// `cvttsd2si`: out of range or NaN is `i32::MIN`.
fn cvtt_f64(x: f64) -> i32 {
    if x.is_nan() || x >= 2147483648.0 || x <= -2147483649.0 {
        i32::MIN
    } else {
        x as i32
    }
}

pub struct GpuProfilerApp;

impl Gui {
    /// `GPUProfilerApp::GPUProfilerApp` 0x14031b320: the GUI's font, 300 x 600.
    pub fn new_gpu_profiler_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "GPU_PROFILER", false);
        self.form_set_title(form, &translate("GPU profiler"));
        self.form_set_size(form, 300.0, 600.0);
        self.set_app(form, Box::new(GpuProfilerApp));
        form
    }
}

impl App for GpuProfilerApp {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `GPUProfilerApp::render` 0x14031b4d0: switches the profiler on (for good), then the rows.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        gui.apps.gpu_profiler_active = true;
        gui.control_render(id, dt, painter);
        let rows = gui.apps.gpu_profile.clone();
        let (Some(first), Some(last)) = (rows.first(), rows.last()) else { return };
        let total = last.last_ts - first.last_ts;
        if rows.len() < 2 || total == 0.0 {
            return;
        }
        let f = gui.controls[id].font.unwrap_or(gui.font);
        gui.fonts[f].set_color(1.0, 1.0, 1.0, 1.0);
        let [x0, y0] = gui.local_to_world(id, [0.0, 0.0]);
        let mut y = 60.0f32;
        for row in &rows {
            let percent = cvtt_f64(1.0 / total * row.last_result * 100.0);
            gui.fonts[f].blit_string(painter, x0 + 10.0, y + y0, &row.name, 1.0, ALIGN_LEFT);
            gui.fonts[f].blit_string(painter, x0 + 170.0, y + y0, &format!("{:.2}", row.last_result), 1.0, ALIGN_LEFT);
            gui.fonts[f].blit_string(painter, x0 + 250.0, y + y0, &format!("{percent}%"), 1.0, ALIGN_LEFT);
            y += 30.0;
        }
        y += 30.0;
        let text = format!("{}: {:.2}", translate("FRAME TIME"), total);
        gui.fonts[f].blit_string(painter, x0 + 10.0, y + y0, &text, 1.0, ALIGN_LEFT);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_percent_is_the_instruction() {
        assert_eq!(super::cvtt_f64(99.9), 99);
        assert_eq!(super::cvtt_f64(-0.5), 0);
        assert_eq!(super::cvtt_f64(3e9), i32::MIN);
        assert_eq!(super::cvtt_f64(f64::NAN), i32::MIN);
    }
}
