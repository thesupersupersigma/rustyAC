// SPDX-License-Identifier: GPL-3.0-or-later

//! `rustyac`: the game on Windows ([`desktop`]). Built for another target (wasm32-wasip1) it
//! is only the replay runner, which is how a wasm build of the physics is held against the
//! desktop's: `rustyac --replay <file.ryin> --headless [--dump-states <file>]`.

#[cfg(windows)]
mod desktop;

#[cfg(windows)]
fn main() {
    desktop::main()
}

#[cfg(not(windows))]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|at| args.get(at + 1));
    let Some(replay) = value("--replay") else {
        eprintln!(
            "this build of rustyac only replays: rustyac --replay <file.ryin> --headless [--dump-states <file>]\n\
             (the game itself is the Windows build; the browser build is crates/rustyac-web)"
        );
        std::process::exit(2);
    };
    let dump = value("--dump-states").map(std::path::Path::new);
    match rustyac_game::run_replay_headless(std::path::Path::new(replay), dump) {
        Ok(steps) => println!("replayed {steps} steps (maths: {:?})", rustyac_math::backend()),
        Err(message) => {
            eprintln!("rustyac: {message}");
            std::process::exit(1);
        }
    }
}
