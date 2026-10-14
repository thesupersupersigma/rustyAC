// SPDX-License-Identifier: GPL-3.0-or-later

//! The GUI oracle's comparison: both sides run a script of `gui_seq` (a process each), then
//! every frame's command log and picture, the state of every control after every frame with
//! the events that fired, and the layout files written are held against each other.

use std::process::Command;

use crate::gui_seq::Outcome;
use crate::Args;

/// One frame of one side: its line for the list, and the whole of it when it was asked for.
pub fn frame_done(args: &Args, name: &str, side: &str, frame: usize, log: &[u8], draws: u64, width: u32, height: u32, pixels: &[u8], outcome: &mut Outcome) -> Result<(), String> {
    let calls = log.iter().filter(|b| **b == b'\n').count();
    outcome.frames.push(format!("{frame} calls={calls} draws={draws} log={:016x} pixels={:016x}", crate::ac::hash(log), crate::ac::hash(pixels)));
    if args.dump.contains(&frame) {
        let file = |kind: &str| args.out.join(format!("{name}.{side}.f{frame}.{kind}"));
        std::fs::write(file("gpulog"), log).map_err(|e| e.to_string())?;
        std::fs::write(file("rgba"), pixels).map_err(|e| e.to_string())?;
        crate::write_png(&file("png"), width, height, pixels)?;
    }
    Ok(())
}

/// `gui-run --side ac|port --gui <sequence>`: one side, its findings written to files.
pub fn run_side(args: &Args) -> Result<(), String> {
    let name = args.gui.as_ref().ok_or("gui-run --side ac|port --gui <sequence>")?;
    let sequence = crate::gui_seq::sequence(name)?;
    crate::root::set_documents(&args.root);
    std::fs::create_dir_all(&args.out).map_err(|e| e.to_string())?;
    let outcome = match args.side.as_str() {
        "ac" => crate::gui_ac::run(args, &sequence, name)?,
        "port" => crate::gui_port::run(args, &sequence, name)?,
        other => return Err(format!("--side {other:?} is not ac or port")),
    };
    let file = |kind: &str| args.out.join(format!("{name}.{}.{kind}", args.side));
    std::fs::write(file("frames"), outcome.frames.join("\n") + "\n").map_err(|e| e.to_string())?;
    std::fs::write(file("state"), &outcome.state).map_err(|e| e.to_string())?;
    for (i, saved) in outcome.saved.iter().enumerate() {
        std::fs::write(file(&format!("acos{i}.ini")), saved).map_err(|e| e.to_string())?;
    }
    println!("{}: {} frames, {} layout files", file("frames").display(), outcome.frames.len(), outcome.saved.len());
    Ok(())
}

fn run_sides(args: &Args, name: &str, dump: &[usize]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut children = Vec::new();
    for side in ["ac", "port"] {
        let mut command = Command::new(&exe);
        command.arg("gui-run").arg("--side").arg(side).arg("--gui").arg(name).arg("--acs").arg(&args.acs).arg("--root").arg(&args.root).arg("--out").arg(&args.out).arg("--size").arg(format!("{}x{}", args.width, args.height));
        if !dump.is_empty() {
            command.arg("--dump").arg(dump.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(","));
        }
        if args.verbose {
            command.arg("--verbose");
        } else {
            command.stdout(std::process::Stdio::null());
        }
        children.push((side, command.spawn().map_err(|e| e.to_string())?));
    }
    for (side, mut child) in children {
        let status = child.wait().map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("the {side} side failed ({status})"));
        }
    }
    Ok(())
}

/// `gui --gui <sequence>`: both sides, then the comparison.
pub fn compare(args: &Args) -> Result<(), String> {
    let name = args.gui.as_ref().ok_or("gui --gui <sequence>")?;
    std::fs::create_dir_all(&args.out).map_err(|e| e.to_string())?;
    {
        let sequence = crate::gui_seq::sequence(name)?;
        crate::gui_seq_setup::prepare_root(args, &sequence)?;
        crate::gui_seq_setup::seed(args, &sequence)?;
        crate::gui_seq_apps::prepare_root(args, name)?;
        crate::gui_seq_tele::prepare_root(args, name)?;
    }
    run_sides(args, name, &args.dump)?;
    let read = |side: &str, kind: &str| {
        let path = args.out.join(format!("{name}.{side}.{kind}"));
        std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
    };
    let text = |side: &str, kind: &str| read(side, kind).map(|b| String::from_utf8_lossy(&b).into_owned());
    let mut failed = false;

    // the frames
    let (fa, fp) = (text("ac", "frames")?, text("port", "frames")?);
    let (la, lp): (Vec<&str>, Vec<&str>) = (fa.lines().collect(), fp.lines().collect());
    let field = |line: &str, key: &str| line.split(' ').find_map(|w| w.strip_prefix(key).map(|v| v.to_string())).unwrap_or_default();
    let number = |line: &str, key: &str| field(line, key).parse::<u64>().unwrap_or(0);
    let calls: u64 = la.iter().map(|l| number(l, "calls=")).sum();
    let draws: u64 = la.iter().map(|l| number(l, "draws=")).sum();
    println!("sequence {name}: {} frames of the game, {} of the port; {calls} calls and {draws} draws of the game", la.len(), lp.len());
    let bad = |key: &str| {
        (0..la.len().max(lp.len()))
            .filter(|i| match (la.get(*i), lp.get(*i)) {
                (Some(a), Some(b)) => field(a, key) != field(b, key) || (key == "log=" && field(a, "calls=") != field(b, "calls=")),
                _ => true,
            })
            .collect::<Vec<usize>>()
    };
    let (log_bad, pixel_bad) = (bad("log="), bad("pixels="));
    if log_bad.is_empty() {
        println!("command log: IDENTICAL in all {} frames", la.len());
    } else {
        failed = true;
        println!("command log: DIFFERENT in {} frames, first frame {}", log_bad.len(), log_bad[0]);
    }
    if pixel_bad.is_empty() {
        println!("pixels: IDENTICAL in all {} frames", la.len());
    } else {
        failed = true;
        println!("pixels: DIFFERENT in {} frames, first frame {}", pixel_bad.len(), pixel_bad[0]);
    }

    // the state of the controls and the events
    let (sa, sp) = (text("ac", "state")?, text("port", "state")?);
    if sa == sp {
        println!("state and events: IDENTICAL ({} lines, {} events)", sa.lines().count(), sa.lines().filter(|l| l.starts_with("event ")).count());
    } else {
        failed = true;
        let (la, lp): (Vec<&str>, Vec<&str>) = (sa.lines().collect(), sp.lines().collect());
        let differing = (0..la.len().max(lp.len())).filter(|i| la.get(*i) != lp.get(*i)).count();
        let mut frame = "";
        for i in 0..la.len().max(lp.len()) {
            let (a, b) = (la.get(i).copied().unwrap_or("<end>"), lp.get(i).copied().unwrap_or("<end>"));
            if a.starts_with("frame ") {
                frame = a;
            }
            if a != b {
                println!("state and events: DIFFERENT in {differing} lines, first in {frame} (line {i}):\n  game: {a}\n  port: {b}");
                let (wa, wb): (Vec<&str>, Vec<&str>) = (a.split(' ').collect(), b.split(' ').collect());
                for (x, y) in wa.iter().zip(wb.iter()) {
                    if x != y {
                        println!("    {x}  /  {y}");
                    }
                }
                break;
            }
        }
    }

    // the layout files
    let mut i = 0;
    loop {
        let kind = format!("acos{i}.ini");
        let (Ok(a), Ok(b)) = (read("ac", &kind), read("port", &kind)) else { break };
        if a == b {
            println!("layout file {i}: IDENTICAL ({} bytes)", a.len());
        } else {
            failed = true;
            let at = a.iter().zip(b.iter()).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
            let line = a[..at.min(a.len())].iter().filter(|c| **c == b'\n').count();
            println!("layout file {i}: DIFFERENT ({} bytes of the game, {} of the port), first at byte {at}, line {}", a.len(), b.len(), line + 1);
        }
        i += 1;
    }

    // the first frame whose log or picture differs, looked at whole
    if let Some(frame) = log_bad.first().or(pixel_bad.first()).copied() {
        if !args.dump.contains(&frame) {
            run_sides(args, name, &[frame])?;
        }
        let (a, b) = (read("ac", &format!("f{frame}.gpulog"))?, read("port", &format!("f{frame}.gpulog"))?);
        let (la, lp): (Vec<&[u8]>, Vec<&[u8]>) = (a.split(|c| *c == b'\n').collect(), b.split(|c| *c == b'\n').collect());
        let differing = (0..la.len().max(lp.len())).filter(|i| la.get(*i) != lp.get(*i)).count();
        println!("frame {frame}: {} calls of the game, {} of the port, {differing} lines differ", la.len() - 1, lp.len() - 1);
        if let Some(i) = (0..la.len().max(lp.len())).find(|i| la.get(*i) != lp.get(*i)) {
            for j in i.saturating_sub(3)..i {
                println!("     {j}: {}", crate::compare::show(la.get(j).copied().unwrap_or(b"")));
            }
            let (x, y) = (la.get(i).copied().unwrap_or(b"<end>"), lp.get(i).copied().unwrap_or(b"<end>"));
            println!("  game {i}: {}", crate::compare::show(x));
            println!("  port {i}: {}", crate::compare::show(y));
            if let Some(detail) = crate::compare::payload_difference(x, y) {
                println!("  {detail}");
            }
            for j in i + 1..(i + 3).min(la.len().max(lp.len())) {
                println!("  game {j}: {}", crate::compare::show(la.get(j).copied().unwrap_or(b"<end>")));
                println!("  port {j}: {}", crate::compare::show(lp.get(j).copied().unwrap_or(b"<end>")));
            }
        }
        let (pa, pb) = (read("ac", &format!("f{frame}.rgba"))?, read("port", &format!("f{frame}.rgba"))?);
        let differing = pa.chunks(4).zip(pb.chunks(4)).filter(|(x, y)| x != y).count();
        println!("frame {frame}: {differing} of {} pixels differ", pa.len() / 4);
    }
    if !crate::gui_seq_apps::compare_files(args, name)? {
        failed = true;
    }
    if failed {
        return Err(format!("{name}: the port does not match the game"));
    }
    Ok(())
}
