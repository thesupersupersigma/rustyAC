// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa's launcher (AssettoCorsa.exe, Kunos Simulazioni).

//! `MainWindow`: the window with the browser in it. The launcher's is a WPF window with CEF 3
//! (`AC.CEF3`); this one is a Win32 window with WebView2. The theme's page is loaded as a
//! `file://` URL, `ac://` requests go to the [`Bridge`] (`ACSchemeHandler`), the default
//! libraries and the theme's scripts are run when the page has loaded
//! (`BrowserLoadEnded` / `JSIncludeDefaultLibs`), and the game is watched the way `acping_Tick`
//! watches acs.exe, with rustyac.exe in its place.
//!
//! Besides the launcher, a script of steps can drive the page (clicks, waits, screenshots) for
//! the checks in docs/port/launcher.md, and a log records every request, event and script error.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::rc::Rc;
use std::time::{Duration, Instant};

use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    take_pwstr, CapturePreviewCompletedHandler, CoreWebView2CustomSchemeRegistration, CoreWebView2EnvironmentOptions, CreateCoreWebView2ControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler, ExecuteScriptCompletedHandler, NavigationCompletedEventHandler, NewWindowRequestedEventHandler,
    WebMessageReceivedEventHandler, WebResourceRequestedEventHandler,
};
use windows::core::{w, Interface, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, IStream, COINIT_APARTMENTTHREADED, STGM_CREATE, STGM_WRITE, STREAM_SEEK_SET};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Shell::{SHCreateMemStream, SHCreateStreamOnFileEx};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::bridge::{split_url, Answer, Bridge, Event};
use crate::folders::AppFolder;
use crate::ini::IniFile;
use crate::{appini, net, setup, sys};

/// How the launcher is started.
pub struct Options {
    /// The game's folder (`AppFolder.AppPath`), read only.
    pub ac_root: String,
    /// The player's folder (`AppFolder.UserDataPath`): rustyac.exe's folder.
    pub docs: String,
    /// The program "drive" starts, and what it is given besides the player's folder.
    pub game: PathBuf,
    pub game_args: Vec<String>,
    pub player_name: String,
    /// The player's real cfg folder the launcher's own is first filled from
    /// ([`setup::seed_from_player`]), only read; `None`: not.
    pub player_cfg: Option<PathBuf>,
    /// Steps that drive the page (see [`Step`]); the launcher closes after the last one.
    pub script: Option<Vec<String>>,
    /// Every request, event and script error, one line each.
    pub log: Option<PathBuf>,
    /// The window's size in pixels, off the screen (the checks); else as launcher.ini says.
    pub offscreen: Option<(i32, i32)>,
    /// `--lobby`: where the server list comes from instead of Kunos' lobby.
    pub lobby: Option<String>,
}

/// rustyAC's controls page (`showcontrols`), not the launcher's.
const CONTROLS_PAGE: &str = include_str!("controls.html");

/// The launcher's theme (`Theme`): the default one, or launcher.ini's `[WINDOW] THEME`.
struct Theme {
    id: String,
    /// `index.html`.
    path: String,
    load_default_libs: bool,
    scripts: Vec<String>,
}

impl Theme {
    fn url(&self) -> String {
        net::uri_path(&self.path)
    }

    fn folder(&self) -> &str {
        &self.path[..self.path.rfind('\\').unwrap_or(0)]
    }
}

/// `GetAvailableThemes`, then the one launcher.ini names. A theme folder without a theme.ini
/// gets one written by the launcher; here such a theme is skipped, as the game's folder is
/// never written.
fn current_theme(f: &AppFolder, launcher_ini: &IniFile) -> Theme {
    let mut themes = Vec::new();
    let default_ini = format!("{}default\\theme.ini", f.default_themes_dir);
    let read_theme = |id: &str, folder: &str, ini_path: &str| {
        let ini = IniFile::new(ini_path);
        Theme {
            id: id.into(),
            path: format!("{folder}\\index.html"),
            load_default_libs: ini.read(Some("theme"), Some("loaddefault")) == "true",
            scripts: ini.read(Some("THEME"), Some("scripts")).split(',').map(str::to_string).collect(),
        }
    };
    if net::file_exists(&default_ini) {
        themes.push(read_theme("default", &format!("{}default", f.default_themes_dir), &default_ini));
    }
    for dir in net::list(&f.default_themes_dir, true).unwrap_or_default() {
        let ini = format!("{dir}\\theme.ini");
        if dir != format!("{}default", f.default_themes_dir) && net::file_exists(&ini) && net::file_exists(&format!("{dir}\\index.html")) {
            themes.push(read_theme(&dir.replace(&f.default_themes_dir, ""), &dir, &ini));
        }
    }
    let selected = launcher_ini.read(Some("WINDOW"), Some("Theme"));
    let i = themes.iter().position(|t| t.id == selected).unwrap_or(0);
    themes.swap_remove(i)
}

/// `GetJSEvent`.
pub fn js_event(name: &str, args: &[&str]) -> String {
    let mut text = args.join(",");
    if !args.is_empty() {
        text = format!("\"{}\"", text.replace(',', "\",\""));
    }
    format!("if(typeof $ != 'undefined') $(document).trigger(\"{name}\", [{text}]);")
}

/// One step of a script.
#[derive(Debug)]
enum Step {
    /// `wait <ms>`.
    Wait(u64),
    /// `js <code>`: run it in the page.
    Js(String),
    /// `click <selector>`: jQuery's click on what the selector finds.
    Click(String),
    /// `until <ms> <expression>`: wait until the expression is true (the check fails after ms).
    Until(u64, String),
    /// `shot <file.png>`: the page as it is now.
    Shot(String),
    /// `game-exit <ms>`: wait until the game that "drive" started has ended.
    GameExit(u64),
    /// `log <text>`.
    Log(String),
}

fn parse_step(line: &str) -> Option<Step> {
    let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
    let ms = |s: &str| s.parse::<u64>().ok();
    Some(match verb {
        "wait" => Step::Wait(ms(rest)?),
        "js" => Step::Js(rest.into()),
        "click" => Step::Click(rest.into()),
        "until" => {
            let (t, expr) = rest.split_once(' ')?;
            Step::Until(ms(t)?, expr.into())
        }
        "shot" => Step::Shot(rest.into()),
        "game-exit" => Step::GameExit(ms(rest)?),
        "log" => Step::Log(rest.into()),
        _ => return None,
    })
}

/// Where a script is.
enum Running {
    Idle,
    WaitUntil(Instant),
    /// An `until` check is out; `Some(true)` when it came back true.
    Checking { deadline: Instant, expr: String, answer: Rc<RefCell<Option<bool>>> },
    Busy(Rc<RefCell<bool>>),
    GameExit(Instant, bool),
}

struct App {
    hwnd: HWND,
    env: ICoreWebView2Environment,
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
    bridge: Bridge,
    theme: Theme,
    options: Options,
    game: Option<Child>,
    ac_active: bool,
    acticks: u32,
    pinging: bool,
    queue: VecDeque<String>,
    /// The online requests out on their threads: number, url, the request and its deferral.
    waiting: Vec<(u64, String, ICoreWebView2WebResourceRequestedEventArgs, ICoreWebView2Deferral)>,
    next_job: u64,
    steps: VecDeque<Step>,
    running: Running,
    log: Option<std::fs::File>,
    /// 1 once the script failed.
    exit_code: i32,
}

const TIMER_PING: usize = 1;
const TIMER_SCRIPT: usize = 2;
/// Handles the host names queued by the request handler, after the request has its answer.
const WM_APP_EVENTS: u32 = WM_APP + 1;
/// An online request's thread is done: its answer is in [`DONE`].
const WM_APP_DONE: u32 = WM_APP + 2;

static DONE: std::sync::Mutex<Vec<(u64, Answer)>> = std::sync::Mutex::new(Vec::new());

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

impl App {
    fn log(&mut self, line: &str) {
        if let Some(file) = &mut self.log {
            let _ = writeln!(file, "{line}");
        }
    }

    fn exec(&self, script: &str) {
        let _ = unsafe { self.webview.ExecuteScript(&HSTRING::from(script), None) };
    }

    /// `ExecuteJS(script, name)`: the script is run as the page's own; an exception it throws
    /// goes to the log.
    fn exec_logged(&self, script: &str, name: &str) {
        let mut quoted = String::new();
        crate::jss::quote(script, &mut quoted);
        let mut source = String::new();
        crate::jss::quote(name, &mut source);
        self.exec(&format!("try{{(0,eval)({quoted})}}catch(e){{window.__acReport&&__acReport('error',{source}+': '+e)}}"));
    }

    fn enter_theme(&self, event: &str) {
        self.exec(&js_event(event, &[]));
        self.exec(&js_event("ACVisible", &[]));
    }

    /// `JSIncludeDefaultLibs`.
    fn include_default_libs(&self) {
        let base = format!("{}.base\\", self.bridge.folders.default_themes_dir);
        for name in ["init.js", "jquery-1.9.1.js", "jquery.easing.js", "jquery.flot.js", "jquery.selectbox.js", "jquery.stickyfloat.js", "jquery.ui.js", "ac.utils.js", "__ACClasses.js", "__AC.js", "__ACUI.js"] {
            // the launcher has the same files as resources, for a .base folder without them
            let text = net::read_all_text(&format!("{base}{name}")).unwrap_or_default();
            self.exec_logged(&text, &format!("ac://js/{name}"));
        }
        for script in &self.theme.scripts {
            let file = format!("{}\\js\\{script}.js", self.theme.folder());
            let text = if net::file_exists(&file) {
                net::read_all_text(&file).unwrap_or_default()
            } else {
                format!("console.log('{script} not found at {}' );", file.replace('\\', "\\\\"))
            };
            self.exec_logged(&text, &format!("ac://theme/{}/js/{script}.js", self.theme.id));
        }
    }

    /// `BrowserLoadEnded`.
    fn load_ended(&mut self) {
        let mut uri = PWSTR::null();
        let url = unsafe { self.webview.Source(&mut uri) }.map(|_| take_pwstr(uri)).unwrap_or_default();
        self.log(&format!("loaded {url}"));
        let url = url.to_lowercase();
        if !url.contains("file://") || !url.contains("index.html") {
            return;
        }
        if self.theme.load_default_libs {
            self.include_default_libs();
        }
    }

    /// The `SchemeEvent`s of a request, after it was answered.
    fn scheme_event(&mut self, host: &str, path: &str) {
        match host {
            "uiready" => {
                self.log("event uiready");
                if !self.pinging {
                    self.pinging = true;
                    unsafe { SetTimer(Some(self.hwnd), TIMER_PING, 2000, None) };
                }
                // the launcher's own screen comes first: the languages to choose from the first
                // time (see WIZARD_JS), then its menu, whose buttons enter the theme. Here the
                // theme is entered at once.
                self.enter_theme("ACIntroEvent");
            }
            "forcetotheme" => self.enter_theme("ACIntroEvent"),
            "setlanguage" => self.log(&format!("event setlanguage {path}")),
            _ => {}
        }
    }

    fn event(&mut self, event: Event) {
        self.log(&format!("event {event:?}"));
        match event {
            Event::Start => {
                if self.game.as_mut().is_some_and(|g| matches!(g.try_wait(), Ok(None))) {
                    return;
                }
                let f = &self.bridge.folders;
                let mut cmd = Command::new(&self.options.game);
                cmd.arg("--documents").arg(f.user_data_path.trim_end_matches('\\')).arg("--out-dir").arg(f.user_out_dir.trim_end_matches('\\'));
                // acs.exe takes car and track from race.ini; rustyac.exe from its command line
                let race = IniFile::new(appini::get(f, "race"));
                // an online join: rustyac.exe takes it from [REMOTE] when it is given no car or track
                let remote = race.read(Some("REMOTE"), Some("ACTIVE")) == "1";
                // a replay of the replays page: the file names its own track and cars
                let replay = race.read(Some("REPLAY"), Some("ACTIVE")) == "1";
                if replay {
                    cmd.arg("--replay").arg(format!("{}{}", f.user_replay_dir, race.read(Some("REPLAY"), Some("FILENAME"))));
                }
                for (flag, key) in [("--car", "MODEL"), ("--track", "TRACK"), ("--layout", "CONFIG_TRACK")].into_iter().filter(|_| !remote && !replay) {
                    let value = race.read(Some("RACE"), Some(key));
                    if !value.is_empty() {
                        cmd.arg(flag).arg(value);
                    }
                }
                cmd.args(&self.options.game_args);
                if let Some(dir) = self.options.game.parent() {
                    cmd.current_dir(dir);
                }
                let line = format!("game {cmd:?}");
                self.log(&line);
                match cmd.spawn() {
                    Ok(child) => self.game = Some(child),
                    Err(e) => self.log(&format!("game failed to start: {e}")),
                }
                self.ping();
            }
            Event::Home => {
                let url = self.theme.url();
                let _ = unsafe { self.webview.Navigate(&HSTRING::from(url)) };
            }
            Event::Exit => unsafe {
                let _ = PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            },
            Event::ContentErrors(list) => {
                // the launcher shows a message box; the log has it
                self.log(&format!("ERRORS with content: {list} view log for details"));
            }
            // the launcher's WPF controls window is rustyAC's own page here, in this window;
            // its Back goes home to the theme
            Event::OpenControls => {
                let _ = unsafe { self.webview.NavigateToString(&HSTRING::from(CONTROLS_PAGE)) };
            }
            Event::StartMusic | Event::StopMusic | Event::SetVolume => {}
        }
    }

    /// `acping_Tick`, watching rustyac.exe instead of a process named acs.
    fn ping(&mut self) {
        let mut active = false;
        if let Some(game) = &mut self.game {
            match game.try_wait() {
                Ok(None) => active = true,
                Ok(Some(status)) => {
                    let line = format!("game exit {status}");
                    self.game = None;
                    self.log(&line);
                }
                Err(_) => self.game = None,
            }
        }
        if self.ac_active != active {
            self.log(if active { "event ACRunningEvent" } else { "event ACStoppedEvent" });
            self.exec(&js_event(if active { "ACRunningEvent" } else { "ACStoppedEvent" }, &[]));
            if !active {
                unsafe {
                    let _ = ShowWindow(self.hwnd, SW_RESTORE);
                    let _ = SetForegroundWindow(self.hwnd);
                }
            }
        }
        self.ac_active = active;
        self.acticks += 1;
        if self.acticks == 6 {
            if !active {
                self.exec(&js_event("ACSoftStoppedEvent", &[]));
            }
            self.exec(&js_event("ACShowroomSoftStopped", &[]));
            self.acticks = 0;
        }
    }

    fn fail(&mut self, why: &str) {
        self.log(&format!("FAIL {why}"));
        self.exit_code = 1;
        self.steps.clear();
    }

    /// Runs the script on until a step has to wait.
    fn step(&mut self) {
        let now = Instant::now();
        match std::mem::replace(&mut self.running, Running::Idle) {
            Running::Idle => {}
            Running::WaitUntil(t) if now < t => return self.running = Running::WaitUntil(t),
            Running::WaitUntil(_) => {}
            Running::Busy(done) => {
                if !*done.borrow() {
                    return self.running = Running::Busy(done);
                }
            }
            Running::Checking { deadline, expr, answer } => {
                let a = *answer.borrow();
                match a {
                    Some(true) => self.log(&format!("ok until {expr}")),
                    _ if now >= deadline => {
                        self.fail(&format!("until {expr}"));
                    }
                    Some(false) => {
                        *answer.borrow_mut() = None;
                        self.check(&expr, &answer);
                        return self.running = Running::Checking { deadline, expr, answer };
                    }
                    None => return self.running = Running::Checking { deadline, expr, answer },
                }
            }
            Running::GameExit(deadline, started) => {
                // the game first has to start (the click's request may still be on its way)
                let running = self.game.is_some() || self.ac_active;
                if !started || running {
                    if now >= deadline {
                        self.fail("game-exit");
                    } else {
                        return self.running = Running::GameExit(deadline, started || running);
                    }
                }
            }
        }
        let Some(step) = self.steps.pop_front() else {
            if self.options.script.is_some() {
                unsafe {
                    let _ = PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                self.options.script = None;
            }
            return;
        };
        self.log(&format!("step {step:?}"));
        match step {
            Step::Wait(ms) => self.running = Running::WaitUntil(now + Duration::from_millis(ms)),
            Step::Js(code) => self.exec_logged(&code, "step"),
            Step::Click(selector) => {
                let mut q = String::new();
                crate::jss::quote(&selector, &mut q);
                self.exec_logged(&format!("var e=$({q});if(!e.length)throw 'nothing matches '+{q};e.first().click();"), "click");
            }
            Step::Until(ms, expr) => {
                let answer = Rc::new(RefCell::new(None));
                self.check(&expr, &answer);
                self.running = Running::Checking { deadline: now + Duration::from_millis(ms), expr, answer };
            }
            Step::Shot(file) => {
                let done = Rc::new(RefCell::new(false));
                let flag = done.clone();
                let stream: windows::core::Result<IStream> = unsafe { SHCreateStreamOnFileEx(&HSTRING::from(file.as_str()), (STGM_CREATE | STGM_WRITE).0, 0x80, true, None) };
                match stream {
                    Ok(stream) => {
                        let keep = stream.clone();
                        let handler = CapturePreviewCompletedHandler::create(Box::new(move |_| {
                            // the file is complete once the stream is let go
                            let _ = unsafe { keep.Commit(windows::Win32::System::Com::STGC_DEFAULT) };
                            *flag.borrow_mut() = true;
                            Ok(())
                        }));
                        let r = unsafe { self.webview.CapturePreview(COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG, &stream, &handler) };
                        if r.is_err() {
                            *done.borrow_mut() = true;
                            self.fail(&format!("shot {file}"));
                        }
                        self.running = Running::Busy(done);
                    }
                    Err(e) => self.fail(&format!("shot {file}: {e}")),
                }
            }
            Step::GameExit(ms) => self.running = Running::GameExit(now + Duration::from_millis(ms), false),
            Step::Log(text) => self.log(&format!("log {text}")),
        }
    }

    fn check(&self, expr: &str, answer: &Rc<RefCell<Option<bool>>>) {
        let answer = answer.clone();
        let handler = ExecuteScriptCompletedHandler::create(Box::new(move |_, result| {
            *answer.borrow_mut() = Some(result == "true");
            Ok(())
        }));
        let _ = unsafe { self.webview.ExecuteScript(&HSTRING::from(format!("(function(){{try{{return !!({expr});}}catch(e){{return false;}}}})()")), &handler) };
    }
}

/// Reports script errors and console lines back to the shell, for the log.
const REPORT_JS: &str = "(function(){var post=function(k,m){try{window.chrome.webview.postMessage(k+' '+m)}catch(e){}};window.__acReport=post;\
window.addEventListener('error',function(e){post('error',e.message+' @ '+e.filename+':'+e.lineno)});\
window.addEventListener('unhandledrejection',function(e){post('error','unhandled rejection '+e.reason)});\
['log','warn','error'].forEach(function(l){var o=console[l];console[l]=function(){post('console.'+l,Array.prototype.map.call(arguments,String).join(' '));return o.apply(console,arguments)}});})();";

/// The original asks the language the first time on its own WPF screen (English, Italiano) and
/// writes launcher.ini [WINDOW] LANGUAGE. The theme's own first-run wizard asks the same two and
/// writes the same key, but the default theme never loads it: loaded with the theme's modules, it
/// shows itself only while that key is empty.
// jQuery's trigger("loadModules") also calls document.onloadModules, which needs no jQuery yet
const WIZARD_JS: &str = "document.onloadModules=function(){__AC.loadModule('FirstRunWizard','body > .modules','en',null,true)};";

extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_SIZE => {
            with_app(|app| unsafe {
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                let _ = app.controller.SetBounds(rect);
            });
            LRESULT(0)
        }
        WM_TIMER => {
            match wparam.0 {
                TIMER_PING => {
                    with_app(App::ping);
                }
                TIMER_SCRIPT => {
                    with_app(App::step);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_APP_EVENTS => {
            while let Some(Some(url)) = with_app(|app| app.queue.pop_front()) {
                let (host, path, _) = split_url(&url);
                with_app(|app| app.scheme_event(&host, &path));
            }
            LRESULT(0)
        }
        WM_APP_DONE => {
            let done: Vec<_> = DONE.lock().unwrap().drain(..).collect();
            for (id, answer) in done {
                let waiting = with_app(|app| {
                    let i = app.waiting.iter().position(|w| w.0 == id)?;
                    let (_, url, args, deferral) = app.waiting.remove(i);
                    app.log(&format!("bridge {url} -> {} {} {}", answer.mime, answer.ok, answer.body.len()));
                    app.queue.push_back(url);
                    Some((args, deferral, app.env.clone()))
                });
                if let Some(Some((args, deferral, env))) = waiting {
                    let _ = respond(&args, &env, &answer);
                    unsafe {
                        let _ = deferral.Complete();
                        let _ = PostMessageW(Some(hwnd), WM_APP_EVENTS, WPARAM(0), LPARAM(0));
                    }
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn read_stream(stream: &IStream) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 65536];
    unsafe {
        let _ = stream.Seek(0, STREAM_SEEK_SET, None);
        loop {
            let mut n = 0u32;
            if stream.Read(buf.as_mut_ptr().cast(), buf.len() as u32, Some(&mut n)).is_err() || n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n as usize]);
        }
    }
    out
}

/// The `ac://` request of a WebResourceRequested event, answered.
fn answer_request(args: &ICoreWebView2WebResourceRequestedEventArgs) -> windows::core::Result<()> {
    let request = unsafe { args.Request()? };
    let mut uri = PWSTR::null();
    unsafe { request.Uri(&mut uri)? };
    let url = take_pwstr(uri);
    // CefWebRequestHandler.OnBeforeResourceLoad: http://ac/... is ac://..., as a POST
    let posted = url.starts_with("http://ac/");
    let url = if posted { url.replacen("http://ac/", "ac://", 1) } else { url };
    let mut method = PWSTR::null();
    unsafe { request.Method(&mut method)? };
    let body = if posted || take_pwstr(method) == "POST" {
        unsafe { request.Content() }.ok().map(|s| String::from_utf8_lossy(&read_stream(&s)).into_owned())
    } else {
        None
    };
    // the online requests wait on the network: answered later, from a thread of their own
    let deferred = with_app(|app| {
        let job = app.bridge.job(&url, body.as_deref())?;
        let deferral = unsafe { args.GetDeferral() }.ok()?;
        app.next_job += 1;
        let (id, hwnd) = (app.next_job, app.hwnd.0 as isize);
        app.waiting.push((id, url.clone(), args.clone(), deferral));
        app.log(&format!("bridge {url} waits"));
        std::thread::spawn(move || {
            let answer = job();
            DONE.lock().unwrap().push((id, answer));
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_APP_DONE, WPARAM(0), LPARAM(0));
            }
        });
        Some(())
    });
    if deferred.flatten().is_some() {
        return Ok(());
    }
    let answered = with_app(|app| {
        let (answer, events) = app.bridge.request(&url, body.as_deref());
        let line = format!("bridge {url}{} -> {} {} {}", body.as_ref().map_or(String::new(), |b| format!(" POST {b}")), answer.mime, answer.ok, answer.body.len());
        app.log(&line);
        for name in app.bridge.not_ported.drain(..).collect::<Vec<_>>() {
            app.log(&format!("not ported: {name}"));
        }
        app.queue.push_back(url.clone());
        unsafe {
            let _ = PostMessageW(Some(app.hwnd), WM_APP_EVENTS, WPARAM(0), LPARAM(0));
        }
        (answer, events, app.env.clone())
    });
    let Some((answer, events, env)) = answered else { return Ok(()) };
    respond(args, &env, &answer)?;
    for event in events {
        with_app(|app| app.event(event));
    }
    Ok(())
}

fn respond(args: &ICoreWebView2WebResourceRequestedEventArgs, env: &ICoreWebView2Environment, answer: &Answer) -> windows::core::Result<()> {
    let stream = unsafe { SHCreateMemStream(Some(&answer.body)) };
    let headers = format!("Content-Type: {}; charset=utf-8\r\nAccess-Control-Allow-Origin: *\r\nCache-Control: no-store", answer.mime);
    let response = unsafe { env.CreateWebResourceResponse(stream.as_ref(), 200, w!("OK"), &HSTRING::from(headers))? };
    unsafe { args.SetResponse(&response) }
}

/// Starts the launcher: the player's folder is set up, the window opens with the theme, and
/// the message loop runs until it is closed. Returns the exit code (1: a script step failed).
pub fn run(options: Options) -> Result<i32, String> {
    let folders = AppFolder::new(&options.ac_root, &options.docs);
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    struct Boxes;
    impl setup::Ask for Boxes {
        fn version_mismatch(&self, text: &str) -> bool {
            unsafe { MessageBoxW(None, &HSTRING::from(text), w!("Version Mismatch"), MB_OKCANCEL | MB_ICONASTERISK) == IDOK }
        }
        fn setup_error(&self, text: &str) {
            unsafe { MessageBoxW(None, &HSTRING::from(text), w!("Error setting up user folders"), MB_OK | MB_ICONHAND) };
        }
    }
    if let Some(player_cfg) = &options.player_cfg {
        setup::seed_from_player(&folders, player_cfg);
    }
    if setup::setup_user_directory(&folders, &Boxes).is_err() {
        return Ok(0);
    }
    let launcher_ini = IniFile::new(format!("{}launcher.ini", folders.user_config_dir));
    let theme = current_theme(&folders, &launcher_ini);

    let hwnd = unsafe {
        let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: w!("rustyACLauncher"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&class);
        let (x, y, w, h, show) = match options.offscreen {
            Some((w, h)) => (-32000, -32000, w, h, SW_SHOWNOACTIVATE),
            None => {
                let full = launcher_ini.read(Some("WINDOW"), Some("fullscreen"));
                (CW_USEDEFAULT, CW_USEDEFAULT, 1600, 900, if full == "1" || full.is_empty() { SW_SHOWMAXIMIZED } else { SW_SHOW })
            }
        };
        let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), w!("rustyACLauncher"), w!("Assetto Corsa Launcher"), WS_OVERLAPPEDWINDOW, x, y, w, h, None, None, Some(instance.into()), None)
            .map_err(|e| e.to_string())?;
        let _ = ShowWindow(hwnd, show);
        hwnd
    };

    // CEF's settings: file pages may read files and call ac://, no web security
    let env_options = CoreWebView2EnvironmentOptions::default();
    let mut args = String::from("--disable-web-security --allow-file-access-from-files --autoplay-policy=no-user-gesture-required");
    if options.offscreen.is_some() {
        args.push_str(" --disable-features=CalculateNativeWinOcclusion --disable-backgrounding-occluded-windows --disable-renderer-backgrounding");
    }
    unsafe { env_options.set_additional_browser_arguments(args) };
    let scheme = CoreWebView2CustomSchemeRegistration::new("ac".into());
    unsafe {
        scheme.set_has_authority_component(true);
        scheme.set_allowed_origins(vec!["*".into()]);
    }
    unsafe { env_options.set_scheme_registrations(vec![Some(scheme.into())]) };
    let env_options: ICoreWebView2EnvironmentOptions = env_options.into();

    // the launcher's browser keeps its data in launcherdata (CefSettings.CachePath)
    let data_dir = folders.user_launcher_data_dir.clone();
    let env = {
        let (tx, rx) = std::sync::mpsc::channel();
        CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
            Box::new(move |handler| unsafe {
                CreateCoreWebView2EnvironmentWithOptions(PCWSTR::null(), &HSTRING::from(data_dir.as_str()), &env_options, &handler).map_err(webview2_com::Error::WindowsError)
            }),
            Box::new(move |result, env| {
                result?;
                tx.send(env).expect("send");
                Ok(())
            }),
        )
        .map_err(|e| format!("WebView2: {e:?}"))?;
        rx.recv().map_err(|e| e.to_string())?.ok_or("WebView2: no environment")?
    };
    let controller = {
        let (tx, rx) = std::sync::mpsc::channel();
        let env2 = env.clone();
        CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
            Box::new(move |handler| unsafe { env2.CreateCoreWebView2Controller(hwnd, &handler).map_err(webview2_com::Error::WindowsError) }),
            Box::new(move |result, controller| {
                result?;
                tx.send(controller).expect("send");
                Ok(())
            }),
        )
        .map_err(|e| format!("WebView2: {e:?}"))?;
        rx.recv().map_err(|e| e.to_string())?.ok_or("WebView2: no controller")?
    };
    let webview = unsafe { controller.CoreWebView2() }.map_err(|e| e.to_string())?;
    unsafe {
        let mut rect = RECT::default();
        let _ = GetClientRect(hwnd, &mut rect);
        let _ = controller.SetBounds(rect);
        let _ = controller.SetIsVisible(true);
        // off the screen the window's pixels are the page's, whatever the screen's scale
        if options.offscreen.is_some() {
            if let Ok(c3) = controller.cast::<ICoreWebView2Controller3>() {
                let _ = c3.SetShouldDetectMonitorScaleChanges(false);
                let _ = c3.SetRasterizationScale(1.0);
            }
        }
        if let Ok(settings) = webview.Settings() {
            let _ = settings.SetIsStatusBarEnabled(false);
            let _ = settings.SetIsZoomControlEnabled(false);
        }
    }

    let log = options.log.as_ref().and_then(|p| std::fs::File::create(p).ok());
    let steps: VecDeque<Step> = options.script.iter().flatten().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).filter_map(|l| parse_step(l.trim())).collect();
    let url = theme.url();
    let scripted = options.script.is_some();
    let host = sys::Windows { player_name: options.player_name.clone() };
    let mut bridge = Bridge::new(folders, Box::new(host));
    bridge.lobby = std::sync::Arc::new(std::sync::Mutex::new(crate::online::Lobby::new(options.lobby.clone())));
    APP.with(|a| {
        *a.borrow_mut() = Some(App {
            hwnd,
            env,
            controller,
            webview: webview.clone(),
            bridge,
            theme,
            options,
            game: None,
            ac_active: false,
            acticks: 0,
            pinging: false,
            queue: VecDeque::new(),
            waiting: Vec::new(),
            next_job: 0,
            steps,
            running: Running::Idle,
            log,
            exit_code: 0,
        })
    });

    unsafe {
        let mut token = 0i64;
        webview.AddWebResourceRequestedFilter(w!("ac://*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL).map_err(|e| e.to_string())?;
        webview.AddWebResourceRequestedFilter(w!("http://ac/*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL).map_err(|e| e.to_string())?;
        let handler = WebResourceRequestedEventHandler::create(Box::new(|_, args| {
            if let Some(args) = args {
                answer_request(&args)?;
            }
            Ok(())
        }));
        webview.add_WebResourceRequested(&handler, &mut token).map_err(|e| e.to_string())?;
        let handler = NavigationCompletedEventHandler::create(Box::new(|_, _| {
            with_app(App::load_ended);
            Ok(())
        }));
        webview.add_NavigationCompleted(&handler, &mut token).map_err(|e| e.to_string())?;
        let handler = WebMessageReceivedEventHandler::create(Box::new(|_, args| {
            if let Some(args) = args {
                let mut text = PWSTR::null();
                if args.TryGetWebMessageAsString(&mut text).is_ok() {
                    let text = take_pwstr(text);
                    with_app(|app| app.log(&text));
                }
            }
            Ok(())
        }));
        webview.add_WebMessageReceived(&handler, &mut token).map_err(|e| e.to_string())?;
        // the theme opens no windows of its own; a link that would is opened in the page
        let handler = NewWindowRequestedEventHandler::create(Box::new(|_, args| {
            if let Some(args) = args {
                let _ = args.SetHandled(true);
            }
            Ok(())
        }));
        webview.add_NewWindowRequested(&handler, &mut token).map_err(|e| e.to_string())?;
        webview.AddScriptToExecuteOnDocumentCreated(&HSTRING::from(REPORT_JS), None).map_err(|e| e.to_string())?;
        webview.AddScriptToExecuteOnDocumentCreated(&HSTRING::from(WIZARD_JS), None).map_err(|e| e.to_string())?;
        webview.Navigate(&HSTRING::from(url)).map_err(|e| e.to_string())?;
        if scripted {
            SetTimer(Some(hwnd), TIMER_SCRIPT, 100, None);
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    let code = APP.with(|a| {
        let mut app = a.borrow_mut().take();
        let code = app.as_ref().map_or(0, |a| a.exit_code);
        if let Some(app) = &mut app {
            app.log("closed");
            let _ = unsafe { app.controller.Close() };
            if let Some(game) = &mut app.game {
                let _ = game.wait();
            }
        }
        code
    });
    Ok(code)
}
