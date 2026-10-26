// SPDX-License-Identifier: GPL-3.0-or-later

//! The developers' console (Task 33) in the GUI oracle: the game's own `Console` on the HUD's
//! `Sim` (its `initCommands` on the first car, a scene camera of the class `CameraForward`,
//! the game's `OptionsManager` reading `system/cfg/options.ini`, the player's car with a `Car`
//! block for `ffMult`), typed into on both sides; what `set` changed in the game's objects is
//! written down every frame (`effects_game.txt`) as the port's actions change its copy
//! (`effects_port.txt`).

use std::sync::Mutex;

use rustyac_gui::console::{Console, ConsoleAction};

use crate::ac::{rd, wr, wstring, Game};
use crate::gui_seq::Op;
use crate::gui_seq_menu::hud;

const DT: f32 = 1.0 / 60.0;
const VA_CONSOLE_CTOR: usize = 0x1_4018_88c0;
const VA_INIT_COMMANDS: usize = 0x1_4018_aa80;
const VA_ON_KEY_CHAR: usize = 0x1_4018_b480;
const VA_ON_KEY_DOWN: usize = 0x1_4018_b620;
const VA_RENDER_HUD: usize = 0x1_4018_c550;
const VA_SHOW: usize = 0x1_4018_c7f0;
const VA_OPTIONS_MANAGER_CTOR: usize = 0x1_4018_d470;
const VA_CAMERA_FORWARD_VFTABLE: usize = 0x1_404e_d0a8;
const VA_EXECUTE_ON_PHYSICS_THREAD: usize = 0x1_4012_1f20;

/// The game's side: the console, the camera, the player's `Car`; the effects' last line.
#[derive(Default)]
struct GameConsole {
    console: usize,
    camera: usize,
    car: usize,
    avatar: usize,
    last: String,
}

static GAME: Mutex<Option<GameConsole>> = Mutex::new(None);

/// The port's side: the console and the game's objects as its actions left them.
struct PortConsole {
    console: Console,
    fov: f32,
    max_layer: f32,
    eyes: [f32; 3],
    ff_mult: f32,
    bias: [f32; 3],
    unix: String,
    last: String,
}

static PORT: Mutex<Option<PortConsole>> = Mutex::new(None);

/// The repository's folder (the sides run in the scratch root): `cardata/` is there.
pub(crate) static REPO: Mutex<std::path::PathBuf> = Mutex::new(std::path::PathBuf::new());

/// The scene camera's field of view at the start.
const FOV: f32 = 56.0;

/// A line of the effects files.
fn effects_line(fov: f32, max_layer: f32, eyes: [f32; 3], ff_mult: f32) -> String {
    let h = |v: f32| format!("{:08x}", v.to_bits());
    format!("fov {} maxLayer {} eyes {} {} {} ffMult {}", h(fov), h(max_layer), h(eyes[0]), h(eyes[1]), h(eyes[2]), h(ff_mult))
}

/// The scratch root (the game's side runs in it; the port's does not).
fn scratch_root() -> std::path::PathBuf {
    crate::root::documents().parent().map(|p| p.to_path_buf()).unwrap_or_default()
}

fn effects_path(side: &str) -> std::path::PathBuf {
    scratch_root().join(format!("effects_{side}.txt"))
}

/// The car files of the console's `saveff`: the repo's car.ini of the player's car, a loose
/// file in the scratch root (the game's side) and in its `port_files` (the port's).
fn car_ini(root: &std::path::Path, unix: &str) -> Result<(), String> {
    let rel = format!("content/cars/{unix}/data/car.ini");
    for to in [root.join(&rel), root.join("port_files").join(&rel)] {
        std::fs::create_dir_all(to.parent().unwrap_or(root)).map_err(|e| e.to_string())?;
        let repo = REPO.lock().unwrap().clone();
        std::fs::copy(repo.join("cardata/ks_ferrari_f2004/car.ini"), &to).map_err(|e| format!("car.ini: {e}"))?;
    }
    Ok(())
}

/// The game's side of a `console.*` op.
pub unsafe fn game_call(game: &Game, what: &str, number: f32, text: &str) -> Result<(), String> {
    let h = crate::gui_ac_hud::hud().ok_or("the HUD first")?;
    let acs = &game.acs;
    match what {
        "console.make" => {
            let sim = h.sim;
            // the scene camera (a CameraForward: initCommands casts it), fov at +8
            let camera = acs.alloc(0x2000);
            wr(camera, 0, acs.va(VA_CAMERA_FORWARD_VFTABLE) as usize);
            wr(camera, 8, FOV);
            wr(sim, 0x238, camera);
            // the OptionsManager (Sim+0xf0), as Sim::Sim makes it
            let ctor: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_OPTIONS_MANAGER_CTOR));
            ctor(sim.add(0xf0));
            // the player's car's physics (ffMult at Car+0x1e4), jobs done at once
            let avatar = h.cars[0];
            let car = acs.alloc(0x4000);
            wr(avatar, 0x1168, car);
            crate::gui_ac_hud::jump(game, VA_EXECUTE_ON_PHYSICS_THREAD, crate::gui_ac_apps::execute_on_physics_thread as *const () as usize);
            let console = acs.alloc(0x128);
            let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_CONSOLE_CTOR));
            ctor(console, sim);
            // Sim::addCar's evNewCarLoaded with the first car: initCommands
            let init: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_INIT_COMMANDS));
            init(console);
            let unix = crate::gui_ac::read_wstring(avatar.add(0x138));
            car_ini(&scratch_root(), &unix)?;
            *GAME.lock().unwrap() = Some(GameConsole { console: console as usize, camera: camera as usize, car: car as usize, avatar: avatar as usize, last: String::new() });
        }
        "console.show" => {
            let c = console()?;
            let show: extern "C" fn(*mut u8, bool) = std::mem::transmute(acs.va(VA_SHOW));
            let shown = rd::<u8>(c, 0x30) != 0;
            show(c, !shown);
        }
        "console.type" => {
            // the characters, then Enter (number 1)
            let c = console()?;
            let f: extern "C" fn(*mut u8, u32) = std::mem::transmute(acs.va(VA_ON_KEY_CHAR));
            for unit in text.encode_utf16() {
                f(c.add(0x60), unit as u32);
            }
            if number != 0.0 {
                f(c.add(0x60), 13);
            }
        }
        "console.char" => {
            let f: extern "C" fn(*mut u8, u32) = std::mem::transmute(acs.va(VA_ON_KEY_CHAR));
            f(console()?.add(0x60), number as u32);
        }
        "console.key" => {
            let event = acs.alloc(0x10);
            wr(event, 0, number as u32);
            let f: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(acs.va(VA_ON_KEY_DOWN));
            f(console()?.add(0x60), event);
        }
        "console.now" => {
            let game_block: *mut u8 = rd(h.sim, 0x8);
            wr(game_block, 0x18, number as f64);
        }
        other => return Err(format!("the game's side knows no {other:?}")),
    }
    let _ = wstring;
    Ok(())
}

unsafe fn console() -> Result<*mut u8, String> {
    GAME.lock().unwrap().as_ref().map(|g| g.console as *mut u8).ok_or("console.make first".to_string())
}

/// The game's console in the HUD pass (last of the HUD objects), and the effects' line.
pub unsafe fn game_render(game: &Game, dt: f32) {
    let mut guard = GAME.lock().unwrap();
    let Some(g) = guard.as_mut() else { return };
    let f: extern "C" fn(*mut u8, f32) = std::mem::transmute(game.acs.va(VA_RENDER_HUD));
    f(g.console as *mut u8, dt);
    let camera = g.camera as *mut u8;
    let line = effects_line(rd(camera, 8), rd(camera, 0x1ac), rd(g.avatar as *mut u8, 0x218), rd(g.car as *mut u8, 0x1e4));
    crate::gui_port_apps::append_effects(&effects_path("game"), Vec::new(), &line, &mut g.last);
}

/// The port's side of a `console.*` op.
pub fn port_call(gui: &mut rustyac_gui::gui::Gui, painter: &mut dyn rustyac_gui::painter::Painter, game_folder: &std::path::Path, unix: &str, what: &str, number: f32, text: &str) -> Result<(), String> {
    let mut guard = PORT.lock().unwrap();
    match what {
        "console.make" => {
            let ini = rustyac_physics::data::ini::IniReader::load(&game_folder.join("system/cfg/options.ini")).ok().filter(|i| i.ready);
            let mut console = Console::new(painter);
            console.init_commands(&rustyac_gui::console::default_options(ini.as_ref()));
            car_ini(&scratch_root(), unix)?;
            *guard = Some(PortConsole { console, fov: FOV, max_layer: 0.0, eyes: [0.0; 3], ff_mult: 0.0, bias: [0.0; 3], unix: unix.to_string(), last: String::new() });
        }
        "console.show" => {
            let p = guard.as_mut().ok_or("console.make first")?;
            let shown = p.console.shown;
            p.console.show(!shown);
        }
        "console.type" => {
            let p = guard.as_mut().ok_or("console.make first")?;
            for unit in text.encode_utf16() {
                p.console.on_key_char(unit as u32);
            }
            if number != 0.0 {
                p.console.on_key_char(13);
            }
        }
        "console.char" => guard.as_mut().ok_or("console.make first")?.console.on_key_char(number as u32),
        "console.key" => guard.as_mut().ok_or("console.make first")?.console.on_key_down(number as u32),
        "console.now" => gui.sim.game_time = number as f64,
        other => return Err(format!("the port's side knows no {other:?}")),
    }
    if let Some(p) = guard.as_mut() {
        carry_out(p);
    }
    Ok(())
}

/// The console's actions on the port's copy of the game's objects.
fn carry_out(p: &mut PortConsole) {
    for action in std::mem::take(&mut p.console.actions) {
        match action {
            ConsoleAction::Set { name, value, .. } => match name.as_str() {
                "fov" => p.fov = value,
                "maxLayer" => p.max_layer = value,
                "driverEye.x" => p.eyes[0] = value,
                "driverEye.y" => p.eyes[1] = value,
                "driverEye.z" => p.eyes[2] = value,
                "ffMult" => p.ff_mult = value,
                "bias0" => p.bias[0] = value,
                "bias1" => p.bias[1] = value,
                "bias2" => p.bias[2] = value,
                _ => {}
            },
            ConsoleAction::Run { name, .. } if name == "saveff" => {
                // the lambda of initCommands: the car's FF mult times 1000, three decimals
                let path = format!("content/cars/{}/data/car.ini", p.unix);
                let value = format!("{:.3}", (p.ff_mult * 1000.0) as f64);
                let file = scratch_root().join("port_files").join(&path);
                if file.is_file() {
                    let old = std::fs::read(&file).ok();
                    let _ = std::fs::write(&file, rustyac_gui::apps::write_profile_string(old.as_deref(), "CONTROLS", "FFMULT", &value));
                    for s in [format!("ffMult ({value}) saved to:"), path, "\n".to_string()] {
                        p.console.print(&s);
                    }
                } else {
                    for s in ["Cannot find file: ".to_string(), path, " to save to".to_string(), "\n".to_string()] {
                        p.console.print(&s);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Task 42: a line the Python apps wrote to the console.
pub fn port_print(line: &str) {
    if let Some(p) = PORT.lock().unwrap().as_mut() {
        p.console.print(line);
        p.console.print("
");
    }
}

/// The port's console in the HUD pass, and the effects' line.
pub fn port_render(gui: &rustyac_gui::gui::Gui, painter: &mut dyn rustyac_gui::painter::Painter) {
    let mut guard = PORT.lock().unwrap();
    let Some(p) = guard.as_mut() else { return };
    p.console.render_hud(painter, gui.screen.0, gui.sim.game_time);
    let line = effects_line(p.fov, p.max_layer, p.eyes, p.ff_mult);
    crate::gui_port_apps::append_effects(&effects_path("port"), Vec::new(), &line, &mut p.last);
}

/// The effects and `car.ini` compared at the end.
pub fn compare_files(args: &crate::Args, name: &str) -> Result<bool, String> {
    if !name.starts_with("console") {
        return Ok(true);
    }
    let mut same = true;
    let rel = "content/cars/ks_ferrari_f2004/data/car.ini";
    for (what, a, b) in [
        ("the game's objects the console set (fov, maxLayer, the eye point, ffMult)", args.root.join("effects_game.txt"), args.root.join("effects_port.txt")),
        (rel, args.root.join(rel), args.root.join("port_files").join(rel)),
    ] {
        match (std::fs::read(&a), std::fs::read(&b)) {
            (Ok(a), Ok(b)) if a == b => println!("file {what}: IDENTICAL ({} bytes)", a.len()),
            (a, b) => {
                same = false;
                println!("file {what}: DIFFERENT\n  game: {:?}\n  port: {:?}", a.map(|x| String::from_utf8_lossy(&x).into_owned()), b.map(|x| String::from_utf8_lossy(&x).into_owned()));
            }
        }
    }
    Ok(same)
}

/// The effects files of a new run.
pub fn prepare_root(args: &crate::Args, name: &str) {
    if name.starts_with("console") {
        for file in ["effects_game.txt", "effects_port.txt"] {
            let _ = std::fs::remove_file(args.root.join(file));
        }
    }
}

/// `console`: made on the first car, shown (Home), typed into: help, set help, sets of each
/// kind, an unknown variable and command, too few words, saveff, a number that is not one;
/// Backspace, the history up and down past its ends, more than 12 lines; hidden and shown.
pub fn sequence(name: &str) -> Option<Vec<Op>> {
    if !name.starts_with("console") {
        return None;
    }
    *REPO.lock().unwrap() = std::env::current_dir().unwrap_or_default();
    let mut o = Vec::new();
    crate::gui_seq_menu::field(&mut o, 3);
    let ty = |o: &mut Vec<Op>, text: &'static str| o.extend([hud("console.type", 1.0, text), Op::Frame(DT)]);
    o.extend([hud("console.make", 0.0, ""), hud("console.now", 1000.0, ""), Op::Frame(DT)]);
    // typing before the console is shown changes nothing
    o.extend([hud("console.type", 1.0, "help"), Op::Frame(DT), hud("console.show", 0.0, ""), Op::Frame(DT), hud("console.now", 2000.0, ""), Op::Frame(DT)]);
    ty(&mut o, "help");
    ty(&mut o, "set help");
    for text in ["set fov 63.5", "set driverEye.y 0.71", "set ffMult 1250", "set AUTOFLIP_RECOVERY 0", "set aniso 4", "set maxLayer 3", "set nope 1", "set fov", "set", "nothing at all", "saveff", "set  driverEye.x   -0.25  ", "help me"] {
        ty(&mut o, text);
    }
    // Backspace; a line of spaces; the history
    o.extend([hud("console.type", 0.0, "set fov 99"), Op::Frame(DT)]);
    for _ in 0..3 {
        o.push(hud("console.char", 8.0, ""));
    }
    o.extend([Op::Frame(DT), hud("console.type", 1.0, "70"), Op::Frame(DT), hud("console.type", 1.0, "   "), Op::Frame(DT)]);
    for key in [0x26u32, 0x26, 0x26, 0x28, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x28, 0x28, 0x28, 0x28, 0x28, 0x28, 0x28, 0x28, 0x28, 0x28, 0x28, 0x28, 0x28, 0x28, 0x28, 0x28, 0x28, 0x28] {
        o.extend([hud("console.key", key as f32, ""), Op::Frame(DT)]);
    }
    o.extend([hud("console.char", 13.0, ""), Op::Frame(DT), hud("console.now", 3000.0, ""), Op::Frame(DT)]);
    // hidden: keys do nothing; shown again
    o.extend([hud("console.show", 0.0, ""), Op::Frame(DT), hud("console.type", 1.0, "set fov 20"), Op::Frame(DT), hud("console.show", 0.0, ""), Op::Frame(DT), Op::Frame(DT)]);
    Some(o)
}
