// SPDX-License-Identifier: GPL-3.0-or-later

//! `text-fallback`: how close the text backend without DirectWrite (`rustyac_gui::ttf`, for the
//! browser) comes to the game's text. Both draw the same lines in white at the same places:
//! the port of the game's FW1/DirectWrite path on WARP, and the fallback into a buffer. Per
//! line: where the ink begins and ends, how much ink there is, and how far the two pictures'
//! coverage is apart.

use rustyac_gui::font::{Font, FontType};
use rustyac_gui::painter::{Painter, ALIGN_LEFT, BLEND_ALPHA, DEPTH_OFF};
use rustyac_gui::ttf::TtfText;
use rustyac_render::gui_paint::GuiRenderer;

use crate::Args;

/// family, italic, bold, size, text
const SAMPLES: [(&str, bool, bool, f32, &str); 12] = [
    ("Segoe UI", false, false, 12.0, "Practice  -  Time left 12:34"),
    ("Segoe UI", false, false, 15.0, "DRIVE THROUGH PENALTY   3 LAPS REMAINING"),
    ("Segoe UI", false, true, 16.0, "Race  -  Lap 2/5   FINAL LAP"),
    ("Segoe UI", false, false, 17.0, "Set on level : 2/3"),
    ("Segoe UI", false, false, 21.0, "Tazio Nuvolari  ABS  TC  Brake Bias 58.5 %"),
    ("Segoe UI", true, false, 21.0, "Ferrari F2004  ks_ferrari_f2004"),
    ("Segoe UI", false, true, 25.0, "You have been disqualified"),
    ("Segoe UI", false, false, 30.0, "CUT DETECTED  LAP WILL NOT COUNT"),
    ("Consolas", false, false, 14.0, "fps 59.9  0x1401b5610  [HEADER] VERSION=2"),
    ("Consolas", false, true, 18.0, "POSX=520.5 POSY=300 SCALE=1.25"),
    ("Orbitron", false, false, 12.0, "Rudolf Caracciola"),
    ("Orbitron", false, true, 18.0, "1 2 3 12 24"),
];

const ROW: usize = 44;
const LEFT: f32 = 24.0;

pub fn run(args: &Args) -> Result<(), String> {
    let mut graphics = crate::gui_port::graphics(args)?;
    let mut renderer = GuiRenderer::new(&graphics);
    let mut ttf = TtfText::default();
    let folder = args.game.join("content/fonts");
    let mut files: Vec<_> = std::fs::read_dir(&folder).map_err(|e| format!("{}: {e}", folder.display()))?.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("ttf"))).collect();
    files.sort();
    for file in &files {
        ttf.add(std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?);
    }

    // the game's text
    graphics.begin_scene();
    graphics.set_screen_space_mode();
    {
        let mut painter = renderer.painter(&mut graphics);
        painter.reset_render_states();
        painter.set_depth_mode(DEPTH_OFF);
        painter.set_blend_mode(BLEND_ALPHA);
        for (i, (family, italic, bold, size, text)) in SAMPLES.iter().enumerate() {
            let kind = match *family {
                "Segoe UI" => FontType::Proportional,
                "Consolas" => FontType::Monospaced,
                _ => FontType::Custom,
            };
            let mut font = Font::new(&mut painter, family, kind, *size, *italic, *bold);
            font.set_color(1.0, 1.0, 1.0, 1.0);
            font.blit_string(&mut painter, LEFT, (8 + i * ROW) as f32, text, 1.0, ALIGN_LEFT);
        }
    }
    let (width, height, pixels) = graphics.kgl.read_screen()?;
    graphics.end_scene();
    let (width, height) = (width as usize, height as usize);
    let background = pixels[0] as f32;
    let game: Vec<u8> = pixels.chunks_exact(4).map(|p| (((p[0] as f32 - background) / (255.0 - background)).clamp(0.0, 1.0) * 255.0 + 0.5) as u8).collect();

    // the fallback's
    let mut soft = vec![0u8; width * height];
    for (i, (family, italic, bold, size, text)) in SAMPLES.iter().enumerate() {
        let Some(wrapper) = ttf.font_wrapper(family, *italic, *bold) else { return Err(format!("no font file of {family} in {}", folder.display())) };
        let image = ttf.string_image(wrapper, text, *size, 0);
        for y in 0..image.height {
            for x in 0..image.width {
                let (px, py) = (LEFT as i32 + image.left + x as i32, (8 + i * ROW) as i32 + image.top + y as i32);
                if px >= 0 && py >= 0 && (px as usize) < width && (py as usize) < height {
                    let at = py as usize * width + px as usize;
                    soft[at] = soft[at].max(image.alpha[y * image.width + x]);
                }
            }
        }
    }

    let mut report = String::from("| font | size | ink box of the game (x0 y0 x1 y1) | the fallback's, less the game's | ink, fallback / game | mean difference of coverage over the ink | ink pixels more than a quarter apart |\n|---|---|---|---|---|---|---|\n");
    let (mut all_diff, mut all_ink, mut all_far) = (0u64, 0u64, 0u64);
    for (i, (family, italic, bold, size, _)) in SAMPLES.iter().enumerate() {
        let rows = (i * ROW).min(height)..((i + 1) * ROW).min(height);
        let bounds = |picture: &[u8]| {
            let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
            for y in rows.clone() {
                for x in 0..width {
                    if picture[y * width + x] > 48 {
                        x0 = x0.min(x as i32);
                        x1 = x1.max(x as i32);
                        y0 = y0.min(y as i32);
                        y1 = y1.max(y as i32);
                    }
                }
            }
            [x0, y0 - (i * ROW) as i32, x1, y1 - (i * ROW) as i32]
        };
        let (a, b) = (bounds(&game), bounds(&soft));
        let (mut ink, mut diff, mut far, mut sum_a, mut sum_b) = (0u64, 0u64, 0u64, 0u64, 0u64);
        for y in rows.clone() {
            for x in 0..width {
                let (p, q) = (game[y * width + x] as i64, soft[y * width + x] as i64);
                sum_a += p as u64;
                sum_b += q as u64;
                if p > 0 || q > 0 {
                    ink += 1;
                    diff += (p - q).unsigned_abs();
                    far += ((p - q).abs() > 64) as u64;
                }
            }
        }
        all_diff += diff;
        all_ink += ink;
        all_far += far;
        let style = match (*italic, *bold) {
            (false, false) => "",
            (true, false) => " italic",
            (false, true) => " bold",
            _ => " bold italic",
        };
        report.push_str(&format!(
            "| {family}{style} | {size} | {} {} {} {} | {:+} {:+} {:+} {:+} | {:.3} | {:.1} % | {:.1} % |\n",
            a[0],
            a[1],
            a[2],
            a[3],
            b[0] - a[0],
            b[1] - a[1],
            b[2] - a[2],
            b[3] - a[3],
            sum_b as f64 / sum_a.max(1) as f64,
            diff as f64 / ink.max(1) as f64 / 2.55,
            far as f64 * 100.0 / ink.max(1) as f64
        ));
    }
    report.push_str(&format!("| all | | | | | {:.1} % | {:.1} % |\n", all_diff as f64 / all_ink.max(1) as f64 / 2.55, all_far as f64 * 100.0 / all_ink.max(1) as f64));
    std::fs::create_dir_all(&args.out).map_err(|e| e.to_string())?;
    let rgba = |picture: &[u8]| picture.iter().flat_map(|v| [*v, *v, *v, 255]).collect::<Vec<u8>>();
    crate::write_png(&args.out.join("text_fallback.game.png"), width as u32, height as u32, &rgba(&game))?;
    crate::write_png(&args.out.join("text_fallback.ttf.png"), width as u32, height as u32, &rgba(&soft))?;
    std::fs::write(args.out.join("text_fallback.md"), &report).map_err(|e| e.to_string())?;
    print!("{report}");
    Ok(())
}
