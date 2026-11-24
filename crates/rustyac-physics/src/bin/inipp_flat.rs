// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C6a-2a: rustyAC's INIpp flatten of each ini in a list (one path a line), in the form
//! `csp_oracle --inipp` prints CSP's: `== <path>` then the text. C6a-2b: with feature `lua` and a
//! CSP dwrite.dll as the second argument (its `ini_std`), Lua on.

fn main() {
    let list = std::env::args().nth(1).expect("usage: inipp_flat <list>");
    let text = std::fs::read_to_string(&list).unwrap_or_else(|e| panic!("{list}: {e}"));
    #[cfg(feature = "lua")]
    let std = std::env::args().nth(2).map(|dll| {
        rustyac_physics::csp::inipp_lua::ini_std(std::path::Path::new(&dll)).unwrap_or_else(|| panic!("{dll}: no ini_std"))
    });
    for path in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        println!("== {path}");
        let path = std::path::Path::new(path);
        #[cfg(feature = "lua")]
        let flat = match &std {
            Some(std) => rustyac_physics::csp::inipp::flatten_lua(path, std),
            None => rustyac_physics::csp::inipp::flatten(path),
        };
        #[cfg(not(feature = "lua"))]
        let flat = rustyac_physics::csp::inipp::flatten(path);
        println!("{}", flat.as_deref().unwrap_or("(not loaded)"));
    }
}
