// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C6a-2a: rustyAC's INIpp flatten of each ini in a list (one path a line), in the form
//! `csp_oracle --inipp` prints CSP's: `== <path>` then the text.

fn main() {
    let list = std::env::args().nth(1).expect("usage: inipp_flat <list>");
    let text = std::fs::read_to_string(&list).unwrap_or_else(|e| panic!("{list}: {e}"));
    for path in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        println!("== {path}");
        let flat = rustyac_physics::csp::inipp::flatten(std::path::Path::new(path));
        println!("{}", flat.as_deref().unwrap_or("(not loaded)"));
    }
}
