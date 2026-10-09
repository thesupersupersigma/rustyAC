// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo run --release -p rustyac-web --example selftest -- <car> <track> [layout] [--steps N]`:
//! the line follower drives N physics steps from the hot-lap start and the car's whole state is
//! printed as one number. The page does the same with `?go=1&autodrive=1&selftest=N` and must
//! show the same number: the browser's physics is the desktop's.

use rustyac_web::session::Session;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let names: Vec<&String> = args.iter().take_while(|a| !a.starts_with("--")).collect();
    let (Some(car), Some(track)) = (names.first(), names.get(1)) else {
        eprintln!("usage: selftest <car> <track> [layout] [--steps N]");
        std::process::exit(2);
    };
    let layout = names.get(2).map(|l| l.as_str()).unwrap_or("");
    let steps: u32 = args.iter().position(|a| a == "--steps").and_then(|at| args.get(at + 1)).and_then(|n| n.parse().ok()).unwrap_or(6667);
    let mut session = match Session::new(car, track, layout, true, "hotlap") {
        Ok(session) => session,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };
    session.input.lock().unwrap().autodrive = true;
    let started = std::time::Instant::now();
    for _ in 0..steps {
        session.advance(rustyac_game::sim::DT as f64 * 1.000_001).expect("a physics step");
    }
    let seconds = started.elapsed().as_secs_f64();
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for word in session.sim.car.car.save_state() {
        for byte in word.to_le_bytes() {
            hash = (hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    let view = session.view();
    println!(
        "{car} on {track} {layout}: {steps} steps (maths {:?}), state {hash:016x}, {:.1} km/h, lap clock {} ms; {:.0} steps/s",
        rustyac_math::backend(),
        view.speed_kmh,
        view.lap.current_ms,
        steps as f64 / seconds
    );
}
