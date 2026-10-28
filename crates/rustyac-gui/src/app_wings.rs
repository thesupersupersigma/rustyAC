// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `FormWings` (0x1c0), the developers' "Wings": a row per wing of the player's car (its angle
//! of attack, ground height, lift and drag areas, angle, yaw) and the totals.
//!
//! `FormWings::FormWings(CarAvatar*)` 0x1402ec890, `render` 0x1402edff0 (its row lambda
//! 0x1402eca90). The outlines it draws in the 3D scene (`onRenderNode` 0x1402edc30) are
//! `rustyac_render::wings::WingsOutline`.

use std::any::Any;

use crate::apps::App;
use crate::control::Id;
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{Painter, ALIGN_LEFT, ALIGN_RIGHT};
use crate::widgets2::format_fixed;
use rustyac_physics::car::aero::WingState;

/// What the app reads of one wing: `physicsInfo.wingData[i]`'s `name`, `chord`, `span`,
/// `position`, `isVertical` and `CarAvatar::wingsStatus[i]`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WingRow {
    pub name: String,
    pub chord: f32,
    pub span: f32,
    pub position: [f32; 3],
    pub is_vertical: bool,
    pub state: WingState,
}

/// `cvttss2si`: out of range or NaN is `i32::MIN`.
pub(crate) fn cvtt(x: f32) -> i32 {
    if x.is_nan() || x >= 2147483648.0 || x < -2147483648.0 {
        i32::MIN
    } else {
        x as i32
    }
}

pub struct WingsApp {
    font: usize,
}

impl Gui {
    /// `FormWings::FormWings(CarAvatar*)` 0x1402ec890: a 12-point monospaced font, 400 x 600.
    pub fn new_wings_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "WINGS", false);
        self.form_set_title(form, &translate("Wings"));
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 12.0, false, false));
        let font = self.fonts.len() - 1;
        self.controls[form].font = Some(font);
        self.form_set_size(form, 400.0, 600.0);
        self.set_app(form, Box::new(WingsApp { font }));
        form
    }
}

impl App for WingsApp {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `FormWings::render` 0x1402edff0.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        gui.control_render(id, dt, painter);
        let f = self.font;
        let mut y = 30.0f32;
        gui.fonts[f].set_color(1.0, 1.0, 0.0, 1.0);
        let [x0, y0] = gui.local_to_world(id, [10.0, y]);
        gui.fonts[f].blit_string(painter, x0, y0, &translate("NAME"), 1.0, ALIGN_LEFT);
        for (dx, head) in [(100.0, "AOA"), (140.0, "GH"), (180.0, "CL"), (220.0, "CD"), (260.0, "\u{b0}"), (300.0, "YAW")] {
            gui.fonts[f].blit_string(painter, x0 + dx, y0, head, 1.0, ALIGN_RIGHT);
        }
        y += 20.0;
        // the lambda's sums: CL area, CD area, drag, lift, the front's lift
        let (mut cl, mut cd, mut drag, mut lift, mut front) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let rows = gui.apps.wings.clone();
        for row in &rows {
            let s = &row.state;
            let v = if s.is_vertical { 0.0 } else { 1.0 };
            gui.fonts[f].set_color(1.0, v, v, 1.0);
            let wing_cl = (row.chord * s.cl) * row.span;
            let wing_cd = (row.chord * s.cd) * row.span;
            if !s.is_vertical {
                cl += wing_cl;
            }
            cd += wing_cd;
            drag += s.drag_kg;
            if !s.is_vertical {
                lift += s.lift_kg;
            }
            front += s.lift_kg * s.front_share;
            let [x, yy] = gui.local_to_world(id, [10.0, y]);
            gui.fonts[f].blit_string(painter, x, yy, &row.name, 1.0, ALIGN_LEFT);
            let cells = [
                (100.0, format_fixed((s.aoa + s.angle) as f64, 2)),
                (140.0, cvtt(s.ground_height * 1000.0).to_string()),
                (180.0, format_fixed(wing_cl as f64, 2)),
                (220.0, format_fixed(wing_cd as f64, 2)),
                (260.0, format_fixed(s.angle as f64, 2)),
                (300.0, format_fixed(s.yaw_angle as f64, 1)),
            ];
            for (dx, text) in cells {
                gui.fonts[f].blit_string(painter, x + dx, yy, &text, 1.0, ALIGN_RIGHT);
            }
            y += 20.0;
        }
        let share = front / lift;
        let efficiency = cl / cd;
        let text = format!(
            "TOT CL:{} @ {}% front CD:{} EFF:{}",
            format_fixed(cl as f64, 2),
            cvtt(share * 100.0),
            format_fixed(cd as f64, 2),
            format_fixed(efficiency as f64, 2)
        );
        gui.fonts[f].set_color(1.0, 1.0, 0.0, 1.0);
        gui.fonts[f].blit_string(painter, x0 + 10.0, y0 + 540.0, &text, 1.0, ALIGN_LEFT);
        let text = format!("TOT(KG) DOWNF: {} DRAG:{}", cvtt(lift), cvtt(drag));
        gui.fonts[f].blit_string(painter, x0 + 10.0, y0 + 555.0, &text, 1.0, ALIGN_LEFT);
    }
}

#[cfg(test)]
mod tests {
    use super::cvtt;

    #[test]
    fn cvtt_is_the_instruction() {
        assert_eq!(cvtt(f32::NAN), i32::MIN);
        assert_eq!(cvtt(-1.9), -1);
        assert_eq!(cvtt(3e9), i32::MIN);
    }
}
