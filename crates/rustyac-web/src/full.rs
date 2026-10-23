// SPDX-License-Identifier: GPL-3.0-or-later

//! Full mode: AC's own renderer (`rustyac_game::render::ac`, kgl on wgpu) drawing the page's
//! game, and its fixed frame: the proof that the page asks kgl for what the desktop asks.
//!
//! The fixed frame: a drive as `Session::new` makes it (no physics step), the renderer made for
//! it, then `frames` frames each 1/60 s after the last, the HUD hidden. The kgl call log runs
//! from the renderer's making to the last frame. The page (`Game::full_proof`) and the native
//! `full_frame` example (kgl on wgpu with `--features wgpu`, on Direct3D 11 without) run the
//! same code.

use std::path::{Path, PathBuf};

use rustyac_game::render::ac::{AcOptions, AcRenderer};
use rustyac_game::render::hud::HudInfo;
use rustyac_game::view::CarView;

use crate::session::Session;

/// A frame's time in the fixed frame.
pub const FIXED_SECONDS: f32 = 1.0 / 60.0;

/// AC's renderer with the session's track and cars, as rustyac.exe makes it without a race.ini
/// (midday, clear, the sun moving). `warp`: Direct3D's or wgpu's WARP natively (the page has its
/// own device); `log`: the kgl call log's names (kgl on wgpu gives them out only then).
pub fn renderer(root: &Path, width: u32, height: u32, session: &Session, warp: bool, log: bool, notes: &mut Vec<String>) -> Result<Box<AcRenderer>, String> {
    let sim = &session.sim;
    let info = sim.car_info();
    let others = sim.opponent_infos();
    // (Direct3D 11's gpu_log is its own command log below kgl: not wanted here)
    let wgpu = cfg!(any(target_arch = "wasm32", feature = "wgpu"));
    // shortcut: rustyac.exe's defaults without a race.ini, not race.ini's sun, weather and groove; read them once the page's races need them
    let options = AcOptions {
        warp,
        gpu_log: (log && wgpu).then(PathBuf::new),
        game: root.to_path_buf(),
        sun_angle: -16.0,
        weather: "3_clear".to_string(),
        skin: None,
        video_exact: false,
        cube_faces: None,
        mirror_size: None,
        sun_animation: Some((1.0, 0.2)),
        groove: None,
        mirror_hq: None,
        world_detail: None,
        render_seed: info.render_seed,
        cube_size: None,
        ideal_line: false,
        session_type: if sim.setup.spawn == "hotlap" { 4 } else { 1 },
        start_lights: None,
        video_ini: None,
        preset: None,
        aa_samples: None,
        post: None,
        fxaa: None,
        motion_blur: None,
        hdr_levels: false,
    };
    let mut renderer = Box::new(AcRenderer::new(width, height, options)?);
    notes.extend(renderer.notes.iter().cloned());
    if let Some(folder) = &info.track_folder {
        notes.push(renderer.load_track(folder, &info.track_layout)?);
    }
    renderer.set_cars_to_be_loaded(1 + others.len());
    match renderer.load_car_info(&info, info.skin.clone())? {
        Some(summary) => {
            notes.push(summary);
            for (number, other) in others.iter().enumerate() {
                match renderer.load_car_info(other, other.skin.clone())? {
                    Some(summary) => notes.push(summary),
                    None => return Err(format!("no 3D model of {} (car {})", other.name, number + 1)),
                }
            }
            let crew = renderer.init_pit_crew();
            if !crew.is_empty() {
                notes.push(crew);
            }
        }
        None => notes.push(format!("no 3D model of {}: no car is drawn", info.name)),
    }
    renderer.finish_loading()?;
    Ok(renderer)
}

/// One frame of the page's game: the player's car first, then the others.
pub fn draw(renderer: &mut AcRenderer, session: &mut Session, view: &CarView, seconds: f32, hud: &HudInfo) {
    let camera = session.camera_frame(view, seconds as f64);
    let world: Vec<CarView> = std::iter::once(*view).chain(session.remote_views()).collect();
    renderer.draw(&world, 0, &session.camera, &camera, hud, seconds);
}

/// The fixed frame of this drive (as it starts: no physics step yet) at this size: the
/// renderer that drew it and the kgl call log.
pub fn fixed_frame(root: &Path, session: &mut Session, width: u32, height: u32, frames: u32, warp: bool, notes: &mut Vec<String>) -> Result<(Box<AcRenderer>, String), String> {
    let view = session.view();
    let setup = &session.sim.setup;
    rustyac_render::kgl_log::begin(&format!("full frame: {} on {} {} from {}, {width} x {height}, {frames} frames of 1/60 s", setup.car, setup.track, setup.layout, setup.spawn));
    let made = renderer(root, width, height, session, warp, true, notes);
    let mut renderer = match made {
        Ok(renderer) => renderer,
        Err(why) => {
            rustyac_render::kgl_log::end();
            return Err(why);
        }
    };
    let hud = HudInfo { hidden: true, ..Default::default() };
    for _ in 0..frames {
        draw(&mut renderer, session, &view, FIXED_SECONDS, &hud);
    }
    Ok((renderer, rustyac_render::kgl_log::end()))
}
