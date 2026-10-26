// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 42: the Python apps. The game side runs acs.exe's own `PythonInterface` (its delay
//! imports of `python33.dll` bound to the player's DLL), the port side `rustyac-python`; both
//! load the same apps (AC's Chat and gMeter, the test apps of `tools/render_oracle/py_apps`)
//! from their own copy of the folders, so the apps' files and `py_log.txt` are compared too.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use crate::ac::{rd, wr, Game};
use crate::gui_seq::Op;
use rustyac_gui::control::Id;
use rustyac_gui::gui::Gui;
use rustyac_gui::painter::Painter;

const VA_PYTHON_INTERFACE: usize = 0x1_4012_4920;
const VA_GAME_UPDATE: usize = 0x1_4024_3010;
const VA_GAME_SHUTDOWN_OBJECT: usize = 0x1_4024_2fc0;
/// the delay import table of `python33.dll`: its names (RVAs of hint + name) and its slots
const VA_PY_NAMES: usize = 0x1_4150_ba90;
const VA_PY_IAT: usize = 0x1_4155_9928;
const PY_IMPORTS: usize = 28;
const VA_CONTROL_SET_VISIBLE: usize = 0x1_4024_51a0;

/// The apps both sides load, in `python.ini`'s order (`MISSING` has no folder; `PY_OFF` is off).
const PYTHON_INI: &str = "[GMETER]\nACTIVE=1\n\n[CHAT]\nACTIVE=1\n\n[ACAPI_UI]\nACTIVE=1\n\n[ACAPI_DATA]\nACTIVE=1\n\n[PY_RAISE]\nACTIVE=1\n\n[PY_SYNTAX]\nACTIVE=1\n\n[MISSING]\nACTIVE=1\n\n[PY_OFF]\nACTIVE=0\n";
/// The files the apps write (compared after the run).
const APP_FILES: [&str; 2] = ["apps/python/acapi_ui/calls.txt", "apps/python/acapi_data/calls.txt"];

fn hud(what: &'static str, number: f32, text: &'static str) -> Op {
    Op::Hud(what, number, text)
}

/// The port's own folder in the root (the two sides run at once).
fn port_root(root: &Path) -> PathBuf {
    root.join("port_py")
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for entry in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?.flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            if entry.file_name() != "__pycache__" {
                copy_tree(&entry.path(), &target)?;
            }
        } else {
            std::fs::copy(entry.path(), &target).map_err(|e| format!("{}: {e}", target.display()))?;
        }
    }
    Ok(())
}

/// Both sides' folders: the apps, `python.ini`, no log or file of an earlier run.
pub fn prepare_root(args: &crate::Args, name: &str) -> Result<(), String> {
    if !name.starts_with("py_") {
        return Ok(());
    }
    let ac = args.acs.parent().ok_or("--acs has no folder")?;
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("py_apps");
    for (cwd, documents) in [(args.root.clone(), args.root.join("documents/Assetto Corsa")), (port_root(&args.root), port_root(&args.root).join("documents/Assetto Corsa"))] {
        let apps = cwd.join("apps/python");
        let _ = std::fs::remove_dir_all(&apps);
        for app in ["Chat", "gMeter", "system"] {
            copy_tree(&ac.join("apps/python").join(app), &apps.join(app))?;
        }
        copy_tree(&fixtures, &apps)?;
        std::fs::create_dir_all(documents.join("cfg")).map_err(|e| e.to_string())?;
        std::fs::write(documents.join("cfg/python.ini"), PYTHON_INI).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(documents.join("logs/py_log.txt"));
    }
    Ok(())
}

/// `py_log.txt` and the apps' files of the two sides.
pub fn compare_files(args: &crate::Args, name: &str) -> Result<bool, String> {
    if !name.starts_with("py_") {
        return Ok(true);
    }
    let mut same = true;
    let port = port_root(&args.root);
    let mut files: Vec<(String, PathBuf, PathBuf)> = vec![("py_log.txt".into(), args.root.join("documents/Assetto Corsa/logs/py_log.txt"), port.join("documents/Assetto Corsa/logs/py_log.txt"))];
    files.extend(APP_FILES.iter().map(|f| (f.to_string(), args.root.join(f), port.join(f))));
    for (what, a, b) in files {
        match (std::fs::read(&a), std::fs::read(&b)) {
            (Ok(a), Ok(b)) if a == b => println!("file {what}: IDENTICAL ({} bytes, {} lines)", a.len(), a.iter().filter(|c| **c == b'\n').count()),
            (a, b) => {
                same = false;
                let (a, b) = (a.map(|x| String::from_utf8_lossy(&x).into_owned()).unwrap_or_default(), b.map(|x| String::from_utf8_lossy(&x).into_owned()).unwrap_or_default());
                let line = a.lines().zip(b.lines()).position(|(x, y)| x != y).unwrap_or(a.lines().count().min(b.lines().count()));
                println!("file {what}: DIFFERENT ({} / {} lines), first at line {}\n  game: {:?}\n  port: {:?}", a.lines().count(), b.lines().count(), line + 1, a.lines().nth(line), b.lines().nth(line));
            }
        }
    }
    Ok(same)
}

// ---- the game's side ----

static PYI: std::sync::Mutex<usize> = std::sync::Mutex::new(0);

/// `py.start`: acs.exe's `PythonInterface` on the apps' stand-in `Sim`; `py.end`: its plugins'
/// `acShutdown`, then `Py_Finalize`.
pub unsafe fn game_call(game: &Game, what: &str) -> Result<(), String> {
    let acs = &game.acs;
    let (game_block, sim) = crate::gui_ac_apps::blocks().ok_or("py.start before app.create")?;
    match what {
        "py.start" => {
            let ac = rustyac_content::install::ac_root().ok_or("no Assetto Corsa folder")?;
            rustyac_python::ffi::load(&ac)?;
            for i in 0..PY_IMPORTS {
                let rva = *(acs.va(VA_PY_NAMES + 8 * i) as *const u64) as usize;
                let name = std::ffi::CStr::from_ptr((acs.va(0x1_4000_0000) + rva + 2) as *const std::ffi::c_char);
                let address = rustyac_python::ffi::address(name);
                if address == 0 {
                    return Err(format!("python33.dll has no {name:?}"));
                }
                acs.patch(acs.va(VA_PY_IAT + 8 * i), &address.to_le_bytes());
            }
            // the pause menu (`isAcLive`: not visible) and the camera manager (mode 0)
            let sim_p = sim as *mut u8;
            for (offset, size) in [(0x110, 0x200), (0x188, 0x200)] {
                if rd::<usize>(sim_p, offset) == 0 {
                    wr(sim_p, offset, acs.alloc(size) as usize);
                }
            }
            // the cars' body nodes get a class (ToeInDeg asks them for their world matrix: the
            // stand-in's matrix at +8)
            let table = acs.alloc(0x100);
            wr(table, 0x40, node_world_matrix as extern "C" fn(*const u8, *mut [f32; 16]) -> *mut [f32; 16] as usize);
            for avatar in crate::gui_ac_apps::avatars() {
                wr(rd::<*mut u8>(avatar as *mut u8, 0x1b8), 0, table as usize);
            }
            // the apps' RaceManager getters of the cars again (the HUD's field took them)
            crate::gui_ac_apps::install_car_getters(game);
            // RaceManager::getHasCompletedFlag (RaceFinished): no car has finished
            crate::gui_ac_hud::jump(game, 0x1_4013_8390, has_completed as extern "C" fn(usize, usize) -> bool as usize);
            // (the camera manager's Sim: isCameraOnBoard asks it for the focused car)
            wr(rd::<*mut u8>(sim_p, 0x188), 0x110, sim_p as usize);
            let pyi = acs.alloc(0x1d8);
            let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_PYTHON_INTERFACE));
            ctor(pyi, sim_p);
            *PYI.lock().unwrap() = pyi as usize;
        }
        // every form the apps made, shown (the player opens them from the apps' bar): the
        // controls whose class is the first one's (an app's `newApp` comes first)
        "py.show" => {
            let pyi = *PYI.lock().unwrap() as *mut u8;
            let (begin, end): (usize, usize) = (rd(pyi, 0x1a8), rd(pyi, 0x1b0));
            let set: extern "C" fn(usize, bool) = std::mem::transmute(acs.va(VA_CONTROL_SET_VISIBLE));
            let controls: Vec<usize> = (begin..end).step_by(8).map(|at| *(at as *const usize)).collect();
            let form_class = controls.first().map(|c| *(*c as *const usize));
            for c in controls {
                if Some(*(c as *const usize)) == form_class {
                    set(c, true);
                }
            }
        }
        "py.end" => {
            let pyi = std::mem::take(&mut *PYI.lock().unwrap());
            if pyi != 0 {
                let shutdown: extern "C" fn(usize, usize) = std::mem::transmute(acs.va(VA_GAME_SHUTDOWN_OBJECT));
                shutdown(game_block, pyi);
                (rustyac_python::ffi::api().finalize)();
            }
        }
        other => return Err(format!("no Python call {other:?}")),
    }
    Ok(())
}

extern "C" fn has_completed(_: usize, _: usize) -> bool {
    false
}

/// `Node::getWorldMatrix` of the stand-in body nodes.
extern "C" fn node_world_matrix(node: *const u8, out: *mut [f32; 16]) -> *mut [f32; 16] {
    unsafe { out.write(rd(node, 8)) };
    out
}

/// A frame: `Game::update` on the interface (its plugins' `acUpdate`).
pub unsafe fn game_frame(game: &Game, dt: f32) {
    let pyi = *PYI.lock().unwrap();
    if pyi == 0 || dt <= 0.0 {
        return;
    }
    let Some((game_block, _)) = crate::gui_ac_apps::blocks() else { return };
    let update: extern "C" fn(usize, usize, f32) = std::mem::transmute(game.acs.va(VA_GAME_UPDATE));
    update(game_block, pyi, dt);
}

// ---- the port's side ----

thread_local! {
    static PYTHON: RefCell<Option<rustyac_python::Python>> = const { RefCell::new(None) };
}

/// The world the stand-in `Sim` has: zeroed cars with a race engineer, no sectors, camera 0.
fn world(gui: &Gui) -> rustyac_python::World {
    // (only a car with a force feedback gain has a race engineer: the player's)
    let cars = gui.apps.cars.iter().map(|c| rustyac_python::CarInfo { min_height: c.user_ff_gain.map(|_| 0.0), ..Default::default() }).collect();
    rustyac_python::World { cars, ..Default::default() }
}

pub fn port_call(gui: &mut Gui, painter: &mut dyn Painter, root: &Path, screen: Id, what: &str) -> Result<(), String> {
    match what {
        "py.start" => {
            let ac = rustyac_content::install::ac_root().ok_or("no Assetto Corsa folder")?;
            let cwd = port_root(root);
            std::env::set_current_dir(&cwd).map_err(|e| format!("{}: {e}", cwd.display()))?;
            let world = world(gui);
            let (fired, widget_events) = (gui.fired.len(), gui.widget_events.len());
            let mut python = rustyac_python::start(&ac, &cwd.join("documents/Assetto Corsa"), &cwd.join("documents/Assetto Corsa/cfg/python.ini"), gui, painter, screen, world)?;
            // (the game's side hooks a control's events at the next frame: the ones of controls
            // made and changed inside acMain are not in its record)
            gui.fired.truncate(fired);
            gui.widget_events.truncate(widget_events);
            drain(&mut python);
            PYTHON.with(|p| *p.borrow_mut() = Some(python));
        }
        "py.show" => PYTHON.with(|p| {
            if let Some(python) = p.borrow().as_ref() {
                let first = python.control(1);
                for handle in 1.. {
                    let Some(id) = python.control(handle) else { break };
                    if first.is_some_and(|f| std::mem::discriminant(&gui.controls[f].kind) == std::mem::discriminant(&gui.controls[id].kind)) {
                        rustyac_python::with_host(gui, painter, |gui, _| gui.set_visible(id, true));
                    }
                }
            }
        }),
        "py.end" => {
            if let Some(python) = PYTHON.with(|p| p.borrow_mut().take()) {
                python.shutdown(gui);
            }
        }
        other => return Err(format!("no Python call {other:?}")),
    }
    Ok(())
}

/// The console lines the apps wrote, into the port's console.
fn drain(python: &mut rustyac_python::Python) {
    for line in python.take_console() {
        crate::gui_seq_console::port_print(&line);
    }
}

pub fn port_frame(gui: &mut Gui, painter: &mut dyn Painter, dt: f32) {
    PYTHON.with(|p| {
        if let Some(python) = p.borrow_mut().as_mut() {
            python.update(gui, painter, dt);
            drain(python);
        }
    });
}

/// A mouse or key event of the script, with the apps' listeners able to reach the GUI.
pub fn port_input<R>(gui: &mut Gui, painter: &mut dyn Painter, f: impl FnOnce(&mut Gui, &mut dyn Painter) -> R) -> R {
    let running = PYTHON.with(|p| p.borrow().is_some());
    if !running {
        return f(gui, painter);
    }
    let r = rustyac_python::with_host(gui, painter, f);
    PYTHON.with(|p| p.borrow_mut().as_mut().map(drain));
    r
}

// ---- the scripts ----

const DT: f32 = 1.0 / 60.0;

/// `py_apps`: the race tapes' session with the console; Python started; frames of the race;
/// clicks on the test app's controls, text typed into Chat; the console shown; Python ended.
pub fn sequence(name: &str) -> Option<Vec<Op>> {
    if !name.starts_with("py_") {
        return None;
    }
    // (the console's car files come from the repository, the sides' folder)
    *crate::gui_seq_console::REPO.lock().unwrap() = std::env::current_dir().unwrap_or_default();
    let mut o = Vec::new();
    crate::gui_seq_apps::race(&mut o, 6);
    crate::gui_seq_menu::field(&mut o, 6);
    o.extend([hud("console.make", 0.0, ""), hud("console.now", 1000.0, "")]);
    o.extend([hud("app.at", 0.0, ""), hud("py.start", 0.0, ""), Op::Frame(DT)]);
    let short = name == "py_quick";
    o.extend([hud("py.show", 0.0, ""), Op::Frame(DT)]);
    crate::gui_seq_apps::play(&mut o, 0, if short { 120 } else { 600 }, if short { 30 } else { 20 });
    // the test app's button, check box and text input clicked (its window at 300, 200), text typed and sent
    for (x, y) in [(320, 260), (320, 284), (330, 334)] {
        o.extend([Op::MouseMove(x, y), Op::MouseDown(x, y), Op::MouseUp(x, y), Op::Frame(DT)]);
    }
    o.extend("hi ac".chars().map(|c| Op::KeyChar(c as u32)));
    o.extend([Op::Frame(DT), Op::KeyChar(13), Op::Frame(DT)]);
    if !short {
        crate::gui_seq_apps::play(&mut o, 600, 1200, 20);
    }
    o.extend([hud("console.show", 0.0, ""), Op::Frame(DT), Op::Frame(DT), hud("py.end", 0.0, "")]);
    Some(o)
}
