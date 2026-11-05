// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 47d: `mirror_check <mirror.tape> <seed>`: the port's F11 virtual mirror
//! (`rustyac_render::mirror::f11`, desktop.rs: a key press once, no headset) on the keys of the
//! game's run (`car_oracle mirror-micro`, tools/car_oracle/src/game/mirror.rs), line by line.

/// The same keys as the game's run (tools/car_oracle/src/game/mirror.rs `keys`).
fn keys(case: u32, seed: u32) -> Vec<bool> {
    let mut x = seed.wrapping_mul(0x9e37_79b9) ^ case.wrapping_mul(0x85eb_ca6b);
    (0..48)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x.is_multiple_of(3)
        })
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("mirror_check <mirror.tape> <seed>");
        std::process::exit(2);
    }
    let game = std::fs::read_to_string(&args[1]).unwrap_or_else(|e| panic!("{}: {e}", args[1]));
    let seed: u32 = args[2].parse().expect("seed");
    let mut port = String::new();
    for case in 0..8u32 {
        let (vr, forced, start) = (case & 1, case >> 1 & 1, case >> 2 & 1);
        port += &format!("case {case} vr {vr} forced {forced} start {start}\n");
        let mut active = start != 0;
        // window.rs drops the key's auto-repeat: one key-down a press, as Trigger::ignoreSubsequentTrue
        let mut held = false;
        for (i, key) in keys(case, seed).into_iter().enumerate() {
            let press = key && !held;
            held = key;
            let said = (press && vr == 0).then(|| rustyac_render::mirror::f11(&mut active, forced != 0));
            port += &format!("{i} key {} active {}\n", key as u8, active as u8);
            if let Some(description) = said {
                let kind = rustyac_gui::hud::MESSAGE_DEFAULT;
                port += &format!("Message({} | {} | {kind})\n", rustyac_gui::locale::ac_translate("VIRTUAL MIRROR"), rustyac_gui::locale::ac_translate(description));
            }
        }
        port += "end\n";
    }
    let messages = game.lines().filter(|l| l.starts_with("Message(")).count();
    match game.lines().zip(port.lines()).position(|(a, b)| a != b) {
        None if game.lines().count() == port.lines().count() => {
            println!("virtual mirror: IDENTICAL ({} lines of the game, 8 cases, {messages} messages)", game.lines().count());
        }
        at => {
            let at = at.unwrap_or(game.lines().count().min(port.lines().count()));
            println!("virtual mirror: DIFFERENT at line {}: game {:?} port {:?}", at + 1, game.lines().nth(at), port.lines().nth(at));
            std::process::exit(1);
        }
    }
}
