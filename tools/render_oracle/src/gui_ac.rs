// SPDX-License-Identifier: GPL-3.0-or-later

//! The game's side of the GUI oracle: acs.exe's own `Font` (FW1FontWrapper and DirectWrite
//! inside it), `ksgui::GUI`, forms, widgets and HUD objects, built by address on the WARP
//! device and driven by the script of `gui_seq`.

use crate::ac::{rd, wr, wstring, Game};
use crate::Args;

const VA_GRAPHICS_BEGIN_SCENE: usize = 0x1_4020_2580; // GraphicsManager::beginScene()
const VA_GRAPHICS_END_SCENE: usize = 0x1_4020_2850; // GraphicsManager::endScene()
const VA_GRAPHICS_SET_SCREEN_SPACE_MODE: usize = 0x1_4020_48e0; // GraphicsManager::setScreenSpaceMode()
const VA_GRAPHICS_RESET_RENDER_STATES: usize = 0x1_4020_4440; // GraphicsManager::resetRenderStates()
const VA_GRAPHICS_SET_BLEND_MODE: usize = 0x1_4020_44e0; // GraphicsManager::setBlendMode(BlendMode)
const VA_GRAPHICS_SET_DEPTH_MODE: usize = 0x1_4020_4580; // GraphicsManager::setDepthMode(DepthMode)
const VA_FONT_WRAPPERS_INIT: usize = 0x1_4001_0f60; // dynamic initializer for Font::fontWrappers
const VA_FONT_CTOR: usize = 0x1_4020_0a60; // Font::Font(const std::wstring&, eFontType, float, bool, bool)
const VA_FONT_BLIT_STRING: usize = 0x1_4020_0e20; // Font::blitString(float, float, const std::wstring&, float, eFontAlign)
const VA_FONT_SET_COLOR: usize = 0x1_4020_15b0; // Font::setColor(float, float, float, float)

/// `font-probe`: a few strings drawn by the game's own `Font`, to see that FW1FontWrapper runs
/// here and what it asks of Direct3D.
pub fn font_probe(args: &Args) -> Result<(), String> {
    crate::root::set_documents(&args.root);
    crate::root::prepare(args)?;
    fonts_into_root(args)?;
    std::env::set_current_dir(&args.root).map_err(|e| format!("{}: {e}", args.root.display()))?;
    std::fs::create_dir_all(&args.out).map_err(|e| e.to_string())?;
    let game = Game::start(args)?;
    unsafe {
        let acs = &game.acs;
        let call0 = |va: usize| {
            let f: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(va));
            f(game.graphics)
        };
        let init: extern "C" fn() = std::mem::transmute(acs.va(VA_FONT_WRAPPERS_INIT));
        init();
        let font_ctor: extern "C" fn(*mut u8, *const u8, i32, f32, bool, bool) -> *mut u8 = std::mem::transmute(acs.va(VA_FONT_CTOR));
        let mut fonts = Vec::new();
        for (kind, italic, bold) in [(0, false, false), (1, false, false), (0, false, true), (0, true, false)] {
            let font = acs.alloc(0x20);
            font_ctor(font, wstring(acs, ""), kind, 1.0, italic, bold);
            println!("font kind {kind} italic {italic} bold {bold}: kid {:#x}", rd::<usize>(font, 0x10));
            fonts.push(font);
        }
        let blit: extern "C" fn(*mut u8, f32, f32, *const u8, f32, i32) = std::mem::transmute(acs.va(VA_FONT_BLIT_STRING));
        let set_color: extern "C" fn(*mut u8, f32, f32, f32, f32) = std::mem::transmute(acs.va(VA_FONT_SET_COLOR));
        for frame in 0..2 {
            rustyac_render::gpulog::begin_capture("font probe");
            call0(VA_GRAPHICS_BEGIN_SCENE);
            call0(VA_GRAPHICS_SET_SCREEN_SPACE_MODE);
            call0(VA_GRAPHICS_RESET_RENDER_STATES);
            let set_mode: extern "C" fn(*mut u8, i32) = std::mem::transmute(acs.va(VA_GRAPHICS_SET_DEPTH_MODE));
            set_mode(game.graphics, 2);
            let set_mode: extern "C" fn(*mut u8, i32) = std::mem::transmute(acs.va(VA_GRAPHICS_SET_BLEND_MODE));
            set_mode(game.graphics, 1);
            for (i, font) in fonts.iter().enumerate() {
                set_color(*font, 1.0, 1.0 - 0.2 * i as f32, 0.5, 1.0);
                wr(*font, 0x4, (i == 2) as u8);
                blit(*font, 100.0, 60.0 + 70.0 * i as f32, wstring(acs, "Hello, Assetto Corsa 0123456789 AVWAT fi"), 14.0 + 10.0 * i as f32, 0);
            }
            blit(fonts[0], 640.0, 400.0, wstring(acs, "centred"), 20.0, 1);
            blit(fonts[0], 640.0, 440.0, wstring(acs, "right"), 20.0, 2);
            let capture = rustyac_render::gpulog::end_capture();
            let (width, height, pixels) = game.read_back()?;
            call0(VA_GRAPHICS_END_SCENE);
            std::fs::write(args.out.join(format!("font_probe.ac.f{frame}.gpulog")), &capture.text).map_err(|e| e.to_string())?;
            std::fs::write(args.out.join(format!("font_probe.ac.f{frame}.rgba")), &pixels).map_err(|e| e.to_string())?;
            crate::write_png(&args.out.join(format!("font_probe.ac.f{frame}.png")), width, height, &pixels)?;
            println!("frame {frame}: {} log bytes, {} draws", capture.text.len(), capture.draws);
        }
    }
    Ok(())
}

/// The game lists `content/fonts/*.ttf` under the current folder (the scratch root): its own
/// font collection, where its Segoe UI and Consolas are.
pub fn fonts_into_root(args: &Args) -> Result<(), String> {
    let to = args.root.join("content/fonts");
    std::fs::create_dir_all(&to).map_err(|e| e.to_string())?;
    let from = args.game.join("content/fonts");
    for entry in std::fs::read_dir(&from).map_err(|e| format!("{}: {e}", from.display()))?.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("ttf")) && !to.join(entry.file_name()).is_file() {
            std::fs::copy(&path, to.join(entry.file_name())).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
