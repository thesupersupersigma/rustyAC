// SPDX-License-Identifier: GPL-3.0-or-later

//! Both sides of a frame, each in a process of its own (the game's objects are never destroyed,
//! and one process has one command log), and the comparison: the frame's command logs line by
//! line, the start-up logs by the calls that make state, and the pictures byte by byte.

use std::process::Command;

use crate::Args;

/// The calls of the start-up that are compared: what makes state objects, views and textures.
/// (The game's start-up also makes a timer query and the buffers of its 2D drawing, and the
/// two sides load models and textures through different paths.)
fn state_lines(log: &[u8]) -> Vec<&[u8]> {
    log.split(|b| *b == b'\n')
        .filter(|l| {
            [&b"dev.CreateBlendState"[..], b"dev.CreateDepthStencilState", b"dev.CreateRasterizerState", b"dev.CreateSamplerState", b"RSSetState", b"PSSetSamplers", b"RSSetViewports"].iter().any(|p| l.starts_with(p))
        })
        .collect()
}

/// A line without its payload of bytes: the call and the names of what it uses, with every
/// run of 16 or more hex digits cut out (for a first, loose look at the structure of a frame).
fn loose(line: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(line.len());
    let mut run = 0usize;
    for &b in line {
        if b.is_ascii_hexdigit() {
            run += 1;
        } else {
            if run >= 16 {
                out.truncate(out.len() - run);
                out.push(b'#');
            }
            run = 0;
        }
        out.push(b);
    }
    if run >= 16 {
        out.truncate(out.len() - run);
        out.push(b'#');
    }
    out
}

fn show(line: &[u8]) -> String {
    let text = String::from_utf8_lossy(line);
    if text.len() > 300 {
        format!("{} … ({} bytes)", &text[..300], text.len())
    } else {
        text.into_owned()
    }
}

/// Where two byte payloads of one call first differ: the offset in the buffer.
fn payload_difference(a: &[u8], b: &[u8]) -> Option<String> {
    let last = |l: &[u8]| l.rsplit(|c| *c == b' ').next().map(|s| s.to_vec()).unwrap_or_default();
    let (pa, pb) = (last(a), last(b));
    if pa.len() != pb.len() || pa.len() < 16 {
        return None;
    }
    let mut offsets = Vec::new();
    for i in (0..pa.len()).step_by(8) {
        let end = (i + 8).min(pa.len());
        if pa[i..end] != pb[i..end] {
            offsets.push(format!("+{:#x}: {} / {}", i / 2, String::from_utf8_lossy(&pa[i..end]), String::from_utf8_lossy(&pb[i..end])));
        }
    }
    Some(format!("{} of {} words differ (game / port): {}", offsets.len(), pa.len() / 8, offsets.iter().take(12).cloned().collect::<Vec<_>>().join(", ")))
}

pub fn compare(args: &Args) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    for side in ["ac", "port"] {
        let mut command = Command::new(&exe);
        command.arg("run").arg("--side").arg(side).arg("--acs").arg(&args.acs).arg("--root").arg(&args.root).args(&args.frame_options);
        if args.verbose {
            command.arg("--verbose");
        }
        let status = command.status().map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("the {side} side failed ({status})"));
        }
    }
    let frame = crate::frames::build(args)?;
    let read = |side: &str, kind: &str| {
        let path = args.out.join(format!("{}.{side}.{kind}", frame.name));
        std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
    };
    let mut failed = false;

    // the start-up: state objects, samplers
    let (init_ac, init_port) = (read("ac", "init.gpulog")?, read("port", "init.gpulog")?);
    let (sa, sp) = (state_lines(&init_ac), state_lines(&init_port));
    if sa == sp {
        println!("start-up: {} state calls, the same on both sides", sa.len());
    } else {
        failed = true;
        println!("start-up: DIFFERENT ({} state calls of the game, {} of the port)", sa.len(), sp.len());
        for (i, (a, b)) in sa.iter().zip(sp.iter()).enumerate() {
            if a != b {
                println!("  first difference at state call {i}:\n    game: {}\n    port: {}", show(a), show(b));
                break;
            }
        }
    }

    // the frame
    let (log_ac, log_port) = (read("ac", "gpulog")?, read("port", "gpulog")?);
    let lines_ac: Vec<&[u8]> = log_ac.split(|b| *b == b'\n').collect();
    let lines_port: Vec<&[u8]> = log_port.split(|b| *b == b'\n').collect();
    let draws = |lines: &[&[u8]]| lines.iter().filter(|l| l.starts_with(b"Draw")).count();
    let same = |a: &[u8], b: &[u8]| if args.loose { loose(a) == loose(b) } else { a == b };
    let mut first = None;
    let mut differing = 0usize;
    for i in 0..lines_ac.len().max(lines_port.len()) {
        let (a, b) = (lines_ac.get(i).copied().unwrap_or(b"<end>"), lines_port.get(i).copied().unwrap_or(b"<end>"));
        if !same(a, b) {
            differing += 1;
            if first.is_none() {
                first = Some(i);
            }
        }
    }
    println!("frame {}: {} calls and {} draws of the game, {} calls and {} draws of the port", frame.name, lines_ac.len() - 1, draws(&lines_ac), lines_port.len() - 1, draws(&lines_port));
    match first {
        None => println!("command log: IDENTICAL{}", if args.loose { " (loose: without the payloads)" } else { "" }),
        Some(i) => {
            failed = true;
            println!("command log: DIFFERENT in {differing} lines, first at line {i}:");
            for j in i.saturating_sub(4)..i {
                println!("     {j}: {}", show(lines_ac.get(j).copied().unwrap_or(b"")));
            }
            let (a, b) = (lines_ac.get(i).copied().unwrap_or(b"<end>"), lines_port.get(i).copied().unwrap_or(b"<end>"));
            println!("  game {i}: {}", show(a));
            println!("  port {i}: {}", show(b));
            if let Some(detail) = payload_difference(a, b) {
                println!("  {detail}");
            }
            for j in i + 1..(i + 4).min(lines_ac.len().max(lines_port.len())) {
                println!("  game {j}: {}", show(lines_ac.get(j).copied().unwrap_or(b"<end>")));
                println!("  port {j}: {}", show(lines_port.get(j).copied().unwrap_or(b"<end>")));
            }
        }
    }

    // the pictures
    let (px_ac, px_port) = (read("ac", "rgba")?, read("port", "rgba")?);
    if px_ac == px_port {
        println!("pixels: IDENTICAL ({} bytes)", px_ac.len());
    } else {
        failed = true;
        let differing = px_ac.chunks(4).zip(px_port.chunks(4)).filter(|(a, b)| a != b).count();
        let worst = px_ac.iter().zip(px_port.iter()).map(|(a, b)| (*a as i32 - *b as i32).abs()).max().unwrap_or(0);
        println!("pixels: DIFFERENT in {differing} of {} pixels, largest channel difference {worst}", px_ac.len() / 4);
    }
    if failed {
        return Err(format!("{}: the port does not match the game", frame.name));
    }
    Ok(())
}
