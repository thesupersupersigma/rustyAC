// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! Assetto Corsa's Python app API: `PythonInterface` 0x140124920 and its `ac` module.
//!
//! The player's own `python33.dll` is loaded at run time (see [`ffi`]); without it the game
//! runs on without Python apps. Apps live in `apps/python/<name>/<name>.py` (relative to the
//! working directory, as in the game: [`start`] is called with the AC folder as the working
//! directory), `Documents/Assetto Corsa/cfg/python.ini` turns them on, and
//! `Documents/Assetto Corsa/logs/py_log.txt` is what `ac.log` writes.
//!
//! The handlers reach the game through a thread-local context: the GUI and the painter the
//! host passes to every entry point ([`Python::update`], [`with_host`], a form's render) and
//! the game's data the apps read ([`World`] and the GUI's `AppLink`).

#![cfg(windows)]

pub mod ac;
pub mod ffi;

use std::cell::{Cell, RefCell};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use ffi::{api, decref, xdecref, Obj};
use rustyac_gui::apps::App;
use rustyac_gui::control::Id;
use rustyac_gui::gui::Gui;
use rustyac_gui::painter::{Painter, Texture};

/// What a car has that the apps read and the GUI's `AppCar` has not.
#[derive(Clone, Debug, Default)]
pub struct CarInfo {
    pub skin: String,
    pub nation_code: String,
    pub ballast_kg: f32,
    pub restrictor: f32,
    /// `raceEngineer->minHeightM` (none: no race engineer)
    pub min_height: Option<f32>,
    pub ai_controlled: bool,
    pub engine_brake_count: i32,
    pub power_controller_count: i32,
    /// `getERSPower().first`: the ERS power controller's index
    pub ers_power: i32,
    /// `physicsInfo.caster`
    pub caster: f64,
    /// the car's cameras (`cameras.size()`)
    pub camera_count: i32,
    /// `RaceManager::getHasCompletedFlag`
    pub completed: bool,
}

/// The game's state the apps read that is not in the GUI's `AppLink`.
#[derive(Clone, Debug, Default)]
pub struct World {
    pub cars: Vec<CarInfo>,
    /// the track's sectors (the splits are padded to this many)
    pub sector_count: usize,
    /// `ACCameraManager::mode`; the drivable camera at the dash
    pub camera_mode: i32,
    pub drivable_dash: bool,
    /// the pause menu is up
    pub paused: bool,
    /// online: the server's name, address, HTTP port and slots (none: offline)
    pub server: Option<(String, String, i32, i32)>,
}

/// What an app asked of the game that the host carries out.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    SetCameraMode(i32),
    /// `setCameraCar(camera, car)`
    SetCameraCar(i32, i32),
    FocusCar(i32),
    /// the player's car's user force feedback gain (0..200)
    SetUserFfGain(f32),
    SendChat(String),
    FreeCameraClearColor([f32; 4]),
    FreeCameraMoveForward(f32),
    FreeCameraMoveRight(f32),
    FreeCameraMoveUpWorld(f32),
    FreeCameraRotatePitch(f32),
    FreeCameraRotateHeading(f32),
    FreeCameraRotateRoll(f32),
    Shutdown,
    /// `restart`: `[RESTART] ACTIVE=1` was written into `cfg/race.ini`, then the game shuts down
    Restart,
}

/// `PythonInterface`'s members.
pub(crate) struct State {
    pub screen: Id,
    pub documents: PathBuf,
    /// `controls` (+0x1a8): every form and control the apps made; `ac`'s handles are 1-based
    pub controls: Vec<Id>,
    /// the forms `newApp` made (`PythonForm`) and their render callbacks
    pub forms: Vec<(Id, Vec<Obj>)>,
    pub textures: Vec<Texture>,
    /// `py_log.txt` (none: it could not be opened, or a character it could not write broke it)
    pub log: Option<File>,
    pub world: World,
    pub console: Vec<String>,
    pub actions: Vec<Action>,
    /// `formCurrentOffset`: where the form the callbacks draw is
    pub offset: (f32, f32),
    /// the chat's listeners (`ACClient::evOnChatMessage`, online only)
    pub chat: Vec<Obj>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static GUI: Cell<Option<*mut Gui>> = const { Cell::new(None) };
    static PAINTER: Cell<Option<*mut (dyn Painter + 'static)>> = const { Cell::new(None) };
}

pub(crate) fn state<T>(f: impl FnOnce(&mut State) -> T) -> T {
    STATE.with(|s| f(s.borrow_mut().as_mut().expect("Python not started")))
}

/// The GUI of the call into Python that is running.
pub(crate) fn gui() -> &'static mut Gui {
    // SAFETY: set by the entry point that called into Python, from a `&mut Gui` it holds for
    // the length of that call; the handlers run inside it.
    unsafe { &mut *GUI.get().expect("no GUI around this call into Python") }
}

/// The painter of the call into Python that is running (none: an input event without one).
pub(crate) fn painter() -> Option<&'static mut dyn Painter> {
    // SAFETY: as for `gui`.
    PAINTER.get().map(|p| unsafe { &mut *p })
}

/// Runs `f` with `gui` and `painter` as the ones the `ac` handlers use: the host feeds the GUI
/// its mouse and key events inside this (an app's listener runs then).
pub fn with_host<R>(gui: &mut Gui, painter: &mut dyn Painter, f: impl FnOnce(&mut Gui, &mut dyn Painter) -> R) -> R {
    let old = (GUI.get(), PAINTER.get());
    GUI.set(Some(gui as *mut Gui));
    // SAFETY: only the lifetime is erased; the pointer is dropped before `painter` ends.
    let p: *mut (dyn Painter + 'static) = unsafe { std::mem::transmute(painter as *mut dyn Painter) };
    PAINTER.set(Some(p));
    let r = f(unsafe { &mut *GUI.get().unwrap() }, unsafe { &mut *p });
    GUI.set(old.0);
    PAINTER.set(old.1);
    r
}

/// Inside a listener: the GUI is the one the event came with.
pub(crate) fn with_gui<R>(gui: &mut Gui, f: impl FnOnce() -> R) -> R {
    let old = GUI.get();
    GUI.set(Some(gui as *mut Gui));
    let r = f();
    GUI.set(old);
    r
}

/// `PythonForm` 0x140124850: a form of an app, whose render callbacks draw after it.
pub struct PythonForm;

impl App for PythonForm {
    fn as_any(&mut self) -> &mut dyn std::any::Any {
        self
    }

    /// `PythonForm::render` 0x140130290.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        gui.control_render(id, dt, painter);
        let rect = gui.controls[id].rect;
        let callbacks = state(|s| {
            s.offset = (rect.left, rect.top);
            s.forms.iter().find(|f| f.0 == id).map(|f| f.1.clone()).unwrap_or_default()
        });
        with_host(gui, painter, |_, _| {
            for cb in callbacks {
                // (the result is kept and the error left as it is, as in the game)
                unsafe { (api().object_call_function)(cb, c"f".as_ptr(), dt as f64) };
            }
        });
    }
}

/// `PyPlugin` 0x140123f50: an app's module and its functions.
struct Plugin {
    name: String,
    update: Obj,
    shutdown: Obj,
}

/// The interpreter and the apps it loaded.
pub struct Python {
    plugins: Vec<Plugin>,
}

/// `PythonInterface::PythonInterface` 0x140124920: loads `python33.dll` from `<ac_root>`,
/// starts the interpreter with the `ac` module and loads the apps `python.ini` turns on
/// (`documents` is the `Documents/Assetto Corsa` folder: `logs/py_log.txt` is written there,
/// `restart` writes `cfg/race.ini`; `python_ini` is its `cfg/python.ini`, or a copy). The
/// working directory must be the game's folder (`apps/python`). An error: the DLL could not be
/// loaded.
pub fn start(ac_root: &Path, documents: &Path, python_ini: &Path, gui: &mut Gui, painter: &mut dyn Painter, screen: Id, world: World) -> Result<Python, String> {
    let api = ffi::load(ac_root)?;
    let logs = documents.join("logs");
    let _ = std::fs::create_dir_all(&logs);
    let log = File::create(logs.join("py_log.txt")).ok();
    STATE.with(|s| *s.borrow_mut() = Some(State { screen, documents: documents.to_path_buf(), controls: Vec::new(), forms: Vec::new(), textures: Vec::new(), log, world, console: Vec::new(), actions: Vec::new(), offset: (0.0, 0.0), chat: Vec::new() }));
    println!("Initialising Python Interface");
    clear_python_cache("apps/python/system/");
    with_host(gui, painter, |_, _| unsafe {
        // PyInterpreter 0x1402cf260
        (api.import_append_inittab)(c"ac".as_ptr(), Some(ac::init));
        (api.initialize)();
        if (api.is_initialized)() == 0 {
            println!("ERROR: PYTHON FATAL ERRROR :: INTERPRETER HAS NOT BEEN INITIALIZED CORRECTLY");
            return Ok(Python { plugins: Vec::new() });
        }
        if run("import sys") == -1 {
            println!("ERROR: PythonInterface :: [ERROR] Python components are not working properly");
        }
        run("sys.dont_write_bytecode = 1");
        run("sys.path.append('apps/python/system')");
        let ini = rustyac_physics::data::ini::IniReader::load(python_ini).unwrap_or_default();
        let folders: Vec<String> = std::fs::read_dir("apps/python").map(|d| d.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
        println!("LOADING {} PY APPS", folders.len());
        let mut plugins = Vec::new();
        for section in ini.sections() {
            let lower = section.to_lowercase();
            let name = folders.iter().find(|f| f.to_lowercase() == lower).cloned().unwrap_or(lower);
            let path = format!("apps/python/{name}/{name}.py");
            // (an app turned off is "not found" too, as in the game; shortcut: a missing ACTIVE
            // reads as 0, untraced in the game; every python.ini AC writes has it)
            if !Path::new(&path).is_file() || ini.get_int(section, "ACTIVE").unwrap_or(0) == 0 {
                println!("ERROR: Python [ERROR] File {path} not found");
                if name != "system" {
                    state(|s| s.console.push(format!("Python [ERROR] File {path} not found in folder")));
                }
                continue;
            }
            clear_python_cache(&format!("apps/python/{name}/"));
            let append = format!("sys.path.append('apps/python/{name}')");
            run(&append);
            match import(&name) {
                Some(module) => plugins.push(plugin(module, &name)),
                None => {
                    // (the game's message names the string it built last, the sys.path line)
                    println!("ERROR: Python ERROR LOADING MODULE :{append}");
                    if name != "system" {
                        state(|s| s.console.push(format!("Python [ERROR] ERROR LOADING MODULE : {append}")));
                    }
                }
            }
        }
        Ok(Python { plugins })
    })
}

unsafe fn run(code: &str) -> i32 {
    (api().run_simple_string_flags)(ffi::cstr(code).as_ptr(), std::ptr::null_mut())
}

pub(crate) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

pub(crate) unsafe fn unicode(text: &str) -> Obj {
    let w = wide(text);
    (api().unicode_from_wide_char)(w.as_ptr(), w.len() as isize)
}

/// `PyModule::PyModule` 0x1402cf4c0.
unsafe fn import(name: &str) -> Option<Obj> {
    println!("Opening PyModule {name}");
    let module = (api().import_import)(unicode(name));
    if module.is_null() {
        println!("ERROR");
    }
    if !(api().err_occurred)().is_null() {
        println!("PyModule :: Errors occurred loading {name}");
        py_error();
        (api().err_clear)();
    }
    (!module.is_null()).then_some(module)
}

/// `PyPlugin::PyPlugin` 0x140123f50: `acMain(1.0)`, then `acUpdate` and `acShutdown` if there.
unsafe fn plugin(module: Obj, name: &str) -> Plugin {
    let api = api();
    println!("Init PyPlugin:{name}");
    let mut p = Plugin { name: name.to_string(), update: std::ptr::null_mut(), shutdown: std::ptr::null_mut() };
    let main = (api.object_get_attr_string)(module, c"acMain".as_ptr());
    if main.is_null() {
        println!("ERROR: acMain not found");
        (api.err_print)();
        return p;
    }
    let r = (api.object_call_function)(main, c"d".as_ptr(), 1.0f64);
    if r.is_null() {
        println!("PyPlugin :: Error in Main function");
        print_err();
    } else {
        for (field, key) in [(&mut p.update, "acUpdate"), (&mut p.shutdown, "acShutdown")] {
            *field = (api.object_get_attr)(module, unicode(key));
            if !field.is_null() {
                println!("Function {key} bind to module");
            }
            py_error();
        }
        decref(r);
    }
    decref(main);
    p
}

/// `PyPlugin::pyError` 0x1401242c0: the pending error, on the standard output only.
unsafe fn py_error() {
    let api = api();
    let (mut t, mut v, mut tb) = (std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
    (api.err_fetch)(&mut t, &mut v, &mut tb);
    let msg = if v.is_null() { "no message".to_string() } else { ac::text(v) };
    println!("{msg}");
    xdecref(v);
    if !tb.is_null() {
        println!("[PY ERROR]: {} ({}) {msg}", tb_file(tb), *(tb as *const u8).add(0x24).cast::<i32>());
        decref(tb);
    }
    xdecref(t);
}

/// The file of a traceback's frame's code (`tb_frame->f_code->co_filename`).
unsafe fn tb_file(tb: Obj) -> String {
    let frame = *(tb as *const u8).add(0x18).cast::<*const u8>();
    let code = *frame.add(0x20).cast::<*const u8>();
    ac::text_keep_error(*code.add(0x60).cast::<Obj>())
}

/// `PythonInterface::printErr` 0x14012fcb0: the pending error to the standard output and the
/// console (only when it has a type, a value and a traceback).
pub(crate) unsafe fn print_err() {
    let api = api();
    let (mut t, mut v, mut tb) = (std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
    (api.err_fetch)(&mut t, &mut v, &mut tb);
    if t.is_null() || v.is_null() || tb.is_null() {
        // (the game leaves these references as they are)
        return;
    }
    // shortcut: the game reads the value as a str only: an exception object raised by Python
    // code (`raise E("...")`) crashes acs.exe here (PyBytes_AsString of a null pointer, seen in
    // the oracle); the port prints an empty message instead
    let msg = if ac::is_str(v) { ac::text_keep_error(v) } else { String::new() };
    let text = format!("[PY ERROR]: {} ({}) {msg}", tb_file(tb), *(tb as *const u8).add(0x24).cast::<i32>());
    println!("{text}");
    state(|s| s.console.push(text));
    decref(v);
    decref(tb);
    decref(t);
}

/// `PythonInterface::writeToLog` 0x140130340: a line of `py_log.txt`. (A `wofstream` with the
/// C locale: a character past ASCII breaks the stream for good.)
pub(crate) fn write_to_log(text: &str) {
    state(|s| {
        let Some(log) = s.log.as_mut() else { return };
        let ascii: String = text.chars().take_while(|c| c.is_ascii()).collect();
        let _ = log.write_all(ascii.as_bytes());
        if ascii.len() != text.len() {
            s.log = None;
            return;
        }
        // (a text-mode stream: endl is CR LF)
        let _ = log.write_all(b"\r\n");
        let _ = log.flush();
    });
}

/// `PythonInterface::clearPythonCache` 0x14012f710: the files of `<dir>__pycache__/`.
fn clear_python_cache(dir: &str) {
    println!("Cleaning Python Cache in path {dir}");
    if let Ok(entries) = std::fs::read_dir(format!("{dir}__pycache__")) {
        for e in entries.flatten() {
            if e.path().is_file() {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

impl Python {
    /// The apps' names, in the order they were loaded.
    pub fn apps(&self) -> Vec<String> {
        self.plugins.iter().map(|p| p.name.clone()).collect()
    }

    /// The GUI control of one of `ac`'s handles (1-based).
    pub fn control(&self, handle: usize) -> Option<Id> {
        state(|s| s.controls.get(handle.wrapping_sub(1)).copied())
    }

    /// The game's state the apps read (the GUI's `AppLink` is the rest).
    pub fn world_mut<T>(&mut self, f: impl FnOnce(&mut World) -> T) -> T {
        state(|s| f(&mut s.world))
    }

    /// `PyPlugin::update` 0x140124670 of every app: `acUpdate(deltaT)` (the game calls it only
    /// when the frame has a time).
    pub fn update(&mut self, gui: &mut Gui, painter: &mut dyn Painter, dt: f32) {
        if dt <= 0.0 {
            return;
        }
        with_host(gui, painter, |_, _| {
            for p in &self.plugins {
                if !p.update.is_null() {
                    unsafe {
                        (api().object_call_function)(p.update, c"d".as_ptr(), dt as f64);
                        print_err();
                    }
                }
            }
        });
    }

    /// A chat line the client heard (lambda 0x140126860): `cb(message, name)`, `name`
    /// "SERVER" for the server's, else the sender's driver ("" for none).
    pub fn on_chat(&mut self, gui: &mut Gui, message: &str, name: &str) {
        let listeners = state(|s| s.chat.clone());
        with_gui(gui, || unsafe {
            for cb in listeners {
                (api().object_call_function)(cb, c"SS".as_ptr(), unicode(message), unicode(name));
            }
        });
    }

    /// The lines the apps wrote to the console since the last call.
    pub fn take_console(&mut self) -> Vec<String> {
        state(|s| std::mem::take(&mut s.console))
    }

    /// What the apps asked of the game since the last call.
    pub fn take_actions(&mut self) -> Vec<Action> {
        state(|s| std::mem::take(&mut s.actions))
    }

    /// `PyPlugin::shutdown` 0x140124630 of every app (`acShutdown()`), then `Py_Finalize`.
    pub fn shutdown(self, gui: &mut Gui) {
        // (no painter: a handler that needs one answers as for a missing file)
        with_gui(gui, || unsafe {
            for p in &self.plugins {
                if !p.shutdown.is_null() {
                    (api().object_call_function)(p.shutdown, c"".as_ptr());
                    print_err();
                }
            }
            (api().finalize)();
        });
    }
}

/// `PythonInterface::getTextureByIndex` 0x14012fbe0 (1-based).
pub(crate) fn texture(index: i32) -> Option<Texture> {
    let t = state(|s| (index >= 1).then(|| s.textures.get(index as usize - 1).cloned()).flatten());
    if t.is_none() {
        println!("ERROR: [PYI ERROR]: Texture index {index} out of range");
    }
    t
}

/// `PythonInterface::getTexture` 0x14012f960: the texture of a file (a new handle every time;
/// 0: no such file).
pub(crate) fn load_texture(path: &str) -> i32 {
    let Some(painter) = painter() else { return 0 };
    if !painter.file_exists(path) {
        return 0;
    }
    let t = painter.get_texture(path);
    state(|s| {
        s.textures.push(t);
        s.textures.len() as i32
    })
}
