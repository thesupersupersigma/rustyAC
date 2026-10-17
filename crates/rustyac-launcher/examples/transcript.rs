// SPDX-License-Identifier: GPL-3.0-or-later

//! The bridge's answers to a list of requests with the fixed host, in a build that also runs in
//! wasm (wasm32-wasip1 under wasmtime): the desktop's transcript and the wasm build's must be
//! byte for byte the same. In wasm the ini calls are always the pure copy.
//!
//!   transcript <AC folder> <player folder> <requests> <out> [--pure-ini]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 4 {
        eprintln!("usage: transcript <AC folder> <player folder> <requests> <out> [--pure-ini]");
        std::process::exit(2);
    }
    if args.iter().any(|a| a == "--pure-ini") {
        rustyac_launcher::ini::use_pure(true);
    }
    let host = Box::new(rustyac_launcher::transcript::Fixed);
    if let Err(e) = rustyac_launcher::transcript::run(&args[0], &args[1], &args[2], &args[3], None, host) {
        eprintln!("transcript: {e}");
        std::process::exit(1);
    }
}
