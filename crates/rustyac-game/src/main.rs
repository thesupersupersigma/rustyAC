//! `rustyac.exe`: see `docs/game/first_drive.md`.

use rustyac_game::cli::Options;

fn run(options: &Options) -> Result<(), String> {
    if let (Some(replay), true) = (&options.replay, options.headless) {
        if options.screenshot.is_none() && options.duration.is_none() {
            let started = std::time::Instant::now();
            let steps = rustyac_game::run_replay_headless(replay, options.dump_states.as_deref())?;
            let seconds = started.elapsed().as_secs_f64();
            // the dump may be going to the standard output
            eprintln!(
                "replayed {steps} steps ({:.1} s of driving) in {seconds:.2} s ({:.1} x real time)",
                steps as f64 * 0.003,
                steps as f64 * 0.003 / seconds.max(1e-9)
            );
            return Ok(());
        }
    }
    Err("only --replay <file> --headless is built so far".to_string())
}

fn main() {
    let options = match Options::parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    if let Err(message) = run(&options) {
        eprintln!("rustyac: {message}");
        std::process::exit(1);
    }
}
