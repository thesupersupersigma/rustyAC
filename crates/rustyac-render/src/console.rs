// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! A console `set` of a variable the scene's objects registered (`ConsoleSet::execute`
//! 0x14018c8f0 finds the first registration of the name): the player's car's own
//! ([`CarAvatar::console_set`]), the track's, the scene camera's and `Sim`'s targets.
//! `bias*` changes nothing (the camera sets the shadow bias again every frame); `minExposure`
//! has no reader; the `hdr*` levels find no `HDR`.

use crate::car::CarAvatar;
use crate::graphics::Graphics;
use crate::post::CameraForwardYebis;

/// False: not a variable of the scene.
pub fn console_set(graphics: &mut Graphics, camera: &mut CameraForwardYebis, car: Option<&mut CarAvatar>, name: &str, value: f32) -> bool {
    let mut car = car;
    if car.as_mut().is_some_and(|car| car.console_set(name, value)) {
        return true;
    }
    match name {
        // TrackAvatar's lambdas: the angle, then GraphicsManager::updateLightingSetttings
        "sunHeading" => {
            graphics.lighting.heading_angle = value;
            graphics.update_lighting_settings();
        }
        "sunPitch" => {
            graphics.lighting.pitch_angle = value;
            graphics.update_lighting_settings();
        }
        // DynamicTrackManager's lambda
        "hideGroove" => graphics.groove_always_hide = value != 0.0,
        // the targets: Camera::maxLayer, CameraShadowMapped::shadowBias, CarAvatar::mirrorPosition
        "maxLayer" => camera.base.camera.max_layer = value,
        "shadowBias" => camera.base.shadow_bias = value,
        // Sim's lambda: the scene camera's axisRenderingMode, 0 to 2 only
        "axisMode" => {
            if (0.0..=2.0).contains(&value) {
                camera.base.camera.axis_rendering_mode = value as i32;
            }
        }
        "mirrorpos.x" | "mirrorpos.y" | "mirrorpos.z" => {
            if let Some(car) = car {
                car.mirror_position[(name.as_bytes()[10] - b'x') as usize] = value;
            }
        }
        _ => return false,
    }
    true
}
