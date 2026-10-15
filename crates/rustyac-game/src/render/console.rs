// SPDX-License-Identifier: GPL-3.0-or-later

//! AC's console in rustyac.exe: its variables and commands registered in the order `Sim::Sim`
//! 0x140192070 and the objects it makes register them (an offline session), and what a `set`
//! or a command asks carried out on rustyAC's counterpart of the game's object, where there
//! is one. The others answer as in the game and change nothing (see
//! `docs/port/apps_telemetry.md`, "The console").

use rustyac_gui::console::{Console, ConsoleAction, VarKind};
use rustyac_physics::car::setup::SetupJob;
use rustyac_physics::data::ini::IniReader;

use super::gui::GuiSetup;

/// The registrations of one car (`CarAvatar::CarAvatar`: `init3D`, `initPhysics`,
/// `initCommonPostPhysics`, then the player's car's own).
fn register_car(console: &mut Console, setup: &GuiSetup, index: usize) {
    use VarKind::*;
    let player = index == 0;
    // init3D: CarLodManager, VisualDamageManager, DriverModel
    console.add_var("lodDebug", Target, 1.0);
    console.add_var("debugDamage", Lambda, 1.0);
    console.add_var("hideHead", Lambda, 1.0);
    if player {
        for n in ["headRoll", "headPitch", "headYaw", "headMaxG", "headFilter"] {
            console.add_var(n, Target, 1.0);
        }
    }
    // initPhysics
    console.add_var("steerAssist", Target, 1.0);
    console.add_command("getSteerAssist", "Returns the current steer assist");
    // initCommonPostPhysics: AnalogInstruments (the player's car), DynamicCarEffects
    if player {
        let analog = setup.data_paths.first().and_then(|data| IniReader::load(&data.join("analog_instruments.ini")).ok()).filter(|ini| ini.ready);
        let has = |section: &str| analog.as_ref().is_some_and(|ini| ini.has_section(section));
        if has("FUEL_INDICATOR") {
            for n in ["FUEL_STEP", "FUEL_ZERO", "FUEL_MIN"] {
                console.add_var(n, Target, 1.0);
            }
        }
        if analog.is_some() {
            for n in ["RPM_STEP", "RPM_ZERO", "SPEED_STEP", "SPEED_ZERO"] {
                console.add_var(n, Target, 1.0);
            }
        }
        for t in 0..4 {
            if has(&format!("TURBO_INDICATOR_{t}")) {
                console.add_var("TURBO_STEP", Target, 1.0);
                console.add_var("TURBO_ZERO", Target, 1.0);
            }
        }
        if has("WATER_TEMP") {
            for n in ["wATER_STEP", "WATER_ZERO", "WATER_MIN"] {
                console.add_var(n, Target, 1.0);
            }
        }
    }
    console.add_var("debugDirt", Lambda, 1.0);
    if player {
        console.add_var("exposure", Target, 1.0);
        console.add_var("editBoxes", Lambda, 1.0);
        console.add_var("editCMesh", Lambda, 1.0);
        console.add_var("restrictor", Lambda, 1.0);
    }
}

/// Every registration of an offline session, in the game's order.
pub fn register(console: &mut Console, setup: &GuiSetup) {
    use VarKind::*;
    console.add_var("axisMode", Lambda, 1.0);
    // TrackAvatar::TrackAvatar, DynamicTrackManager
    console.add_var("sunHeading", Lambda, 1.0);
    console.add_var("sunPitch", Lambda, 1.0);
    console.add_var("hideGroove", Lambda, 1.0);
    // the first car, then initCommands (Sim::addCar's evNewCarLoaded), then the others
    register_car(console, setup, 0);
    let options = IniReader::load(&setup.game.join("system/cfg/options.ini")).ok().filter(|ini| ini.ready);
    console.init_commands(&rustyac_gui::console::default_options(options.as_ref()));
    for i in 1..setup.cars.len() {
        register_car(console, setup, i);
    }
    // FormAI (made in every session)
    for n in ["aiSteerGain", "aiPush", "aiLookAhead", "aiLookAheadSpeed", "aiSteerGain"] {
        console.add_var(n, Lambda, 1.0);
    }
    // CameraOnBoardFree
    for n in ["freeCamFov", "freeCamDof", "freeCamDofFocus", "freeCamExp"] {
        console.add_var(n, Target, 1.0);
    }
    console.add_var("freeF5", Lambda, 1.0);
    // Sim::initHDRLevels and the rest of Sim::Sim
    console.add_var("minExposure", Target, 1.0);
    for n in ["mirrorpos.x", "mirrorpos.y", "mirrorpos.z", "shadowBias"] {
        console.add_var(n, Target, 1.0);
    }
    for n in ["hdrHPLevel", "hdrBloomLevel", "hdrBloomBaseLevel"] {
        console.add_var(n, Lambda, 1.0);
    }
    for i in 0..5 {
        console.add_var(&format!("bodyDamageLevel{i}"), Lambda, 1.0);
    }
    console.add_var("engineDamageLevel", Lambda, 1.0);
    // SteamInterface
    console.add_command("rSsA", "Resets Steam Achievements");
    // ACClientVotingManager::ACClientVotingManager 0x140054460 (online; vote_kick's help is
    // vote_restart's: the game's two lambdas were merged into one)
    if setup.online {
        console.add_command("vote_next", "Start skip to next session voting");
        console.add_command("vote_restart", "Start restart session voting");
        console.add_command("vote_kick", "Start restart session voting");
    }
}

/// What the console asked, carried out: on the session's own objects (the player's cockpit
/// view, the physics thread's jobs), the commands' lines. True: `exit`.
pub fn carry_out(gui: &mut super::gui::GameGui) -> bool {
    let mut exit = false;
    for action in std::mem::take(&mut gui.console.actions) {
        match action {
            ConsoleAction::Set { name, value, .. } => match name.as_str() {
                // the eye point (CarAvatar::driverEyesPosition), as Onboard settings moves it
                "driverEye.x" | "driverEye.y" | "driverEye.z" => {
                    let k = (name.as_bytes()[10] - b'x') as usize;
                    gui.apps.onboard.0[k] = value;
                    gui.apps.onboard_changed = true;
                }
                // the cockpit view's field of view (see the report: the game's scene camera
                // takes CameraOnBoard::onBoardFov again every frame in the cockpit)
                "fov" => {
                    gui.apps.onboard.1 = value;
                    gui.apps.onboard_changed = true;
                }
                // CarAvatar::setFFMult's job; Car::steerAssist
                "ffMult" => gui.apps.ffb_jobs.push(SetupJob::SetFfMult { value }),
                "steerAssist" => gui.apps.ffb_jobs.push(SetupJob::SetSteerAssist { value }),
                _ => {}
            },
            ConsoleAction::Run { name, line, index } => run(gui, &name, &line, index),
            ConsoleAction::Exit => exit = true,
        }
    }
    exit
}

/// A command other than `set`.
fn run(gui: &mut super::gui::GameGui, name: &str, line: &str, index: usize) {
    match name {
        "getSteerAssist" => {
            // the registration's car: the n-th getSteerAssist is the n-th car's
            let car = gui.console.commands[..index].iter().filter(|c| c.name == "getSteerAssist").count();
            let value = gui.gui.apps.cars.get(car).map_or(0.0, |c| c.steer_assist);
            gui.console.print("Steer Assist:");
            gui.console.print_float(value);
            gui.console.print("\n");
        }
        "saveff" => {
            // the player's car's FF mult times 1000, three decimals, into its car.ini when the
            // car has a loose one (rustyAC: into its own cfg copy of it)
            let unix = gui.setup.cars.first().map(|c| c[2].clone()).unwrap_or_default();
            let path = format!("content/cars/{unix}/data/car.ini");
            let mult = gui.gui.apps.cars.first().map_or(0.0, |c| c.ff_mult);
            let value = format!("{:.3}", (mult * 1000.0) as f64);
            if gui.setup.game.join(&path).is_file() {
                gui.gui.apps.actions.push(rustyac_gui::apps::AppAction::WriteProfileString { file: path.clone(), section: "CONTROLS".into(), key: "FFMULT".into(), value: value.clone() });
                for s in [format!("ffMult ({value}) saved to:"), path, "\n".to_string()] {
                    gui.console.print(&s);
                }
            } else {
                for s in ["Cannot find file: ".to_string(), path, " to save to".to_string(), "\n".to_string()] {
                    gui.console.print(&s);
                }
            }
        }
        "refresh_instruments" => {
            // (rustyAC's dashboard reads digital_instruments.ini once, at the car's loading)
            gui.console.print("REFRESHING digital_instruments.ini");
            gui.console.print("\n");
        }
        "testCrash" => {
            // ksGenerateCrash: the game raises the SEH exception 0x29a and its crash handler
            // ends it; rustyAC says so and goes on
            println!("console: testCrash: Assetto Corsa would raise the exception 0x0000029a here (ksGenerateCrash) and close with its crash report; rustyAC carries on");
        }
        // the voting manager's commands (0x140054c40, 0x140054cc0, 0x1400549c0): vote_kick's
        // name is what follows the first space; the last car of that driver wins; nobody:
        // still a vote (guid -1)
        "vote_next" => gui.console_vote(0, -1),
        "vote_restart" => gui.console_vote(1, -1),
        "vote_kick" => {
            let name = line.find(' ').map_or(line, |pos| &line[pos + 1..]);
            let guid = gui.gui.sim.race.cars.iter().enumerate().rfind(|(_, car)| car.driver_name == name).map_or(-1, |(i, _)| i as i32);
            gui.console_vote(2, guid);
        }
        // SteamInterface::resetAchievements: rustyAC has no Steam achievements
        "rSsA" => println!("RESETTING STEAM ACHIEVEMENTS"),
        _ => {}
    }
}
