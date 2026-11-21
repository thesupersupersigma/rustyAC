// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C5i-b: the whole car, step by step. `csp_oracle --compare-states <game> <port> <cars>`
//! reads two files of `CarPhysicsState`s in the game's 0xb70-byte layout (the drive's
//! `states.bin`, `rustyac --physics-states`), every step every car, and prints for every value
//! (`member[k]`: line k of a member's `{:#?}` text) the first step where the two differ, earliest
//! first. Rust prints an f32 so that it reads back to the same bits, so equal text is equal bits.

use rustyac_physics::car_physics_state::{CarPhysicsState, GAME_SIZE};

/// The first step a value differs, with the game's and the port's line.
type Difference = Option<(usize, String, String)>;

/// The members of a state with their text, in declaration order.
fn members(state: &CarPhysicsState) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in format!("{state:#?}").lines().skip(1) {
        let top = line.starts_with("    ") && !line.starts_with("     ");
        match line.trim_start().split_once(": ") {
            Some((name, _)) if top => out.push((name.to_string(), line.to_string())),
            _ => {
                if let Some(last) = out.last_mut() {
                    last.1.push('\n');
                    last.1.push_str(line);
                }
            }
        }
    }
    out
}

pub fn compare(game: &str, port: &str, cars: usize) {
    let read = |path: &str| std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let (a, b) = (read(game), read(port));
    let per_step = GAME_SIZE * cars;
    let steps = a.len().min(b.len()) / per_step;
    println!("states: {} steps in {game}, {} in {port}, {cars} cars, {steps} compared", a.len() / per_step, b.len() / per_step);
    for car in 0..cars {
        // per value (a line of a member's text, `member[k]`): the first step it differs, both lines
        let mut first: Vec<(String, Difference)> = Vec::new();
        for step in 0..steps {
            let at = step * per_step + car * GAME_SIZE;
            let state = |bytes: &[u8]| CarPhysicsState::from_game_bytes(&bytes[at..at + GAME_SIZE]).expect("a state");
            let lines = |s: &CarPhysicsState| -> Vec<(String, String)> {
                members(s).into_iter().flat_map(|(name, text)| {
                    text.lines().enumerate().map(|(k, l)| (format!("{name}[{k}]"), l.trim().to_string())).collect::<Vec<_>>()
                }).collect()
            };
            let (x, y) = (lines(&state(&a)), lines(&state(&b)));
            if first.is_empty() {
                first = x.iter().map(|(key, _)| (key.clone(), None)).collect();
            }
            for ((slot, (_, l)), (_, r)) in first.iter_mut().zip(&x).zip(&y) {
                if slot.1.is_none() && l != r {
                    slot.1 = Some((step, l.clone(), r.clone()));
                }
            }
        }
        let mut differ: Vec<_> = first.iter().filter_map(|(key, d)| d.as_ref().map(|d| (d.0, key, &d.1, &d.2))).collect();
        differ.sort_by_key(|d| d.0);
        println!("car {car}: {} of {} values equal at every step", first.len() - differ.len(), first.len());
        for (step, key, l, r) in differ {
            println!("  step {step} {key}: game {l} port {r}");
        }
    }
}
