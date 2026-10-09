// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo run --release -p rustyac-web --example files -- <car> <track> [layout]`: the files a
//! drive reads (R) and the files the page fetches before it (P), with sizes.

use rustyac_web::content;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(car), Some(track)) = (args.first(), args.get(1)) else {
        eprintln!("usage: files <car> <track> [layout]");
        std::process::exit(2);
    };
    let layout = args.get(2).map(String::as_str).unwrap_or("");
    let Some(root) = rustyac_content::install::ac_root() else {
        eprintln!("{}", rustyac_content::install::not_found_hint());
        std::process::exit(2);
    };
    let read = match content::files_read_by_drive(&root, car, track, layout) {
        Ok(read) => read,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };
    let planned = content::files_of_drive(&root, car, track, layout);
    let mut all: Vec<_> = read.iter().chain(&planned).filter(|f| f.starts_with(&root)).cloned().collect();
    all.sort();
    all.dedup();
    let (mut read_bytes, mut planned_bytes) = (0u64, 0u64);
    for file in &all {
        let size = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
        let (r, p) = (read.contains(file), planned.contains(file));
        read_bytes += if r { size } else { 0 };
        planned_bytes += if p { size } else { 0 };
        println!("{}{} {:>12} {}", if r { 'R' } else { '-' }, if p { 'P' } else { '-' }, size, file.strip_prefix(&root).unwrap_or(file).display());
    }
    println!("read {:.1} MB, planned {:.1} MB", read_bytes as f64 / 1_048_576.0, planned_bytes as f64 / 1_048_576.0);
}
