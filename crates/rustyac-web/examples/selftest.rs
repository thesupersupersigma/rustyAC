// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo run --release -p rustyac-web --example selftest -- <car> <track> [layout] [--steps N]
//! [--keys <list>]`: the line follower drives N physics steps from the hot-lap start and the
//! car's whole state is printed as one number. The page does the same with
//! `?go=1&autodrive=1&selftest=N` and must show the same number: the browser's physics is the
//! desktop's.
//!
//! `--keys KeyT@300,Shift+KeyT@600,KeyY@900,BracketRight@1200,BracketLeft@1500` presses the
//! page's command keys on the way (T after 300 steps, and so on); the page takes the same list
//! as `&keys=...` and sends the keys through its own key handler. After each press the line
//! the display shows of the aids is printed, as the page reports it.

use rustyac_web::session::Session;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let names: Vec<&String> = args.iter().take_while(|a| !a.starts_with("--")).collect();
    let (Some(car), Some(track)) = (names.first(), names.get(1)) else {
        eprintln!("usage: selftest <car> <track> [layout] [--steps N] [--keys KeyT@300,Shift+KeyT@600,...]");
        std::process::exit(2);
    };
    let layout = names.get(2).map(|l| l.as_str()).unwrap_or("");
    let option = |name: &str| args.iter().position(|a| a == name).and_then(|at| args.get(at + 1));
    let steps: u32 = option("--steps").and_then(|n| n.parse().ok()).unwrap_or(6667);
    let mut keys: Vec<(u32, String)> = option("--keys")
        .map(|list| {
            list.split(',')
                .filter(|item| !item.is_empty())
                .map(|item| {
                    let (name, at) = item.split_once('@').unwrap_or((item, "0"));
                    (at.parse::<u32>().unwrap_or(0).min(steps), name.to_string())
                })
                .collect()
        })
        .unwrap_or_default();
    keys.sort_by_key(|(at, _)| *at);
    let mut session = match Session::new(car, track, layout, true, "hotlap") {
        Ok(session) => session,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };
    session.input.lock().unwrap().autodrive = true;
    let started = std::time::Instant::now();
    let run = |session: &mut Session, count: u32| {
        for _ in 0..count {
            session.advance(rustyac_game::sim::DT as f64 * 1.000_001).expect("a physics step");
        }
    };
    let shown = |session: &Session| {
        let view = session.view();
        format!("{} | {} | {}", view.tc_text(), view.abs_text(), view.bias_text())
    };
    let mut done = 0;
    for (at, name) in &keys {
        // (a key at the step of the one before it comes a step later: that step has been run)
        run(&mut session, at.saturating_sub(done));
        done = done.max(*at);
        let before = shown(&session);
        if !session.press(name) {
            eprintln!("{name}: not a command key of the page");
            std::process::exit(2);
        }
        // the command runs before the next physics step
        if done < steps {
            run(&mut session, 1);
            done += 1;
        }
        println!("{name} after {at} steps: {before}  ->  {}   note: {}", shown(&session), session.view().aid_note().unwrap_or(""));
    }
    run(&mut session, steps - done);
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
