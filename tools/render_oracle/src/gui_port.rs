// SPDX-License-Identifier: GPL-3.0-or-later

//! The port's side of the GUI oracle: `rustyac_render`'s fonts and the widgets of `rustyac-gui`
//! drawn through the same command log as the game's.

use rustyac_render::graphics::{Graphics, VideoSettings};
use rustyac_render::kgl::DeviceOptions;

use crate::Args;

pub fn graphics(args: &Args) -> Result<Graphics, String> {
    let window = unsafe { crate::ac::hidden_window(args.width, args.height)? };
    let video = VideoSettings {
        aa_samples: crate::root::profile().aa_samples,
        width: args.width as i32,
        height: args.height as i32,
        is_fullscreen: false,
        v_sync: false,
        anisotropic: crate::root::profile().anisotropic,
        aa_quality: 0,
        shadow_map_size: crate::root::profile().shadow_map_size,
        fps_cap_ms: 0.0,
        world_detail: crate::root::profile().world_detail,
        smoke: Some(crate::root::profile().smoke),
        mirror_size: crate::root::profile().mirror_size,
        mirror_smoke: crate::root::profile().mirror_smoke,
        pp_hdr_enabled: crate::root::profile().post,
        pp_fxaa: crate::root::profile().fxaa,
        ldr_levels: crate::root::profile().ldr_levels,
        triple_buffer: false,
    };
    Graphics::new(video, DeviceOptions { warp: !crate::ac::hardware(), window: Some(window), log: !crate::timing() }, &args.game)
}

/// `font-probe --side port`: the strings of `gui_ac::font_probe`, drawn by the port.
pub fn font_probe(args: &Args) -> Result<(), String> {
    std::fs::create_dir_all(&args.out).map_err(|e| e.to_string())?;
    let mut graphics = graphics(args)?;
    let mut fonts = rustyac_render::fw1::Fonts::new(&args.game);
    let mut list = Vec::new();
    for (family, italic, bold) in [("Segoe UI", false, false), ("Consolas", false, false), ("Segoe UI", false, true), ("Segoe UI", true, false)] {
        list.push(fonts.get_font_wrapper(&graphics.kgl, family, italic, bold).ok_or("no font")?);
    }
    let wide = |text: &str| text.encode_utf16().collect::<Vec<u16>>();
    let pack = |r: f32, g: f32, b: f32, a: f32| ((a * 255.0) as i64 as u32 & 0xff) << 24 | ((b * 255.0) as i64 as u32 & 0xff) << 16 | ((g * 255.0) as i64 as u32 & 0xff) << 8 | ((r * 255.0) as i64 as u32 & 0xff);
    for frame in 0..2 {
        rustyac_render::gpulog::begin_capture("font probe");
        graphics.begin_scene();
        graphics.set_screen_space_mode();
        graphics.reset_render_states();
        graphics.set_depth_mode(2);
        graphics.set_blend_mode(1);
        for (i, font) in list.iter().enumerate() {
            let color = pack(1.0, 1.0 - 0.2 * i as f32, 0.5, 1.0);
            let (x, y, size) = (100.0, 60.0 + 70.0 * i as f32, 14.0 + 10.0 * i as f32);
            let text = wide("Hello, Assetto Corsa 0123456789 AVWAT fi");
            if i == 2 {
                fonts.blit_string(&graphics.kgl, *font, 2.0 + x, 2.0 + y, &text, size, 0, 0xff00_0000);
            }
            fonts.blit_string(&graphics.kgl, *font, x, y, &text, size, 0, color);
        }
        let color = pack(1.0, 1.0, 0.5, 1.0);
        fonts.blit_string(&graphics.kgl, list[0], 640.0, 400.0, &wide("centred"), 20.0, 2, color);
        fonts.blit_string(&graphics.kgl, list[0], 640.0, 440.0, &wide("right"), 20.0, 1, color);
        let capture = rustyac_render::gpulog::end_capture();
        let (width, height, pixels) = graphics.kgl.read_screen()?;
        graphics.end_scene();
        std::fs::write(args.out.join(format!("font_probe.port.f{frame}.gpulog")), &capture.text).map_err(|e| e.to_string())?;
        std::fs::write(args.out.join(format!("font_probe.port.f{frame}.rgba")), &pixels).map_err(|e| e.to_string())?;
        crate::write_png(&args.out.join(format!("font_probe.port.f{frame}.png")), width, height, &pixels)?;
        println!("frame {frame}: {} log bytes, {} draws", capture.text.len(), capture.draws);
    }
    Ok(())
}
