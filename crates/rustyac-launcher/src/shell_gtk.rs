// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa's launcher (AssettoCorsa.exe, Kunos Simulazioni).

//! `MainWindow` off Windows: a GTK window with WebKitGTK 4.1 through `wry`, doing what the
//! WebView2 one (`shell.rs`, Windows) does:
//! the theme's page as a `file://` URL, `ac://` requests answered by the [`Bridge`], the default
//! libraries and the theme's scripts run when the page has loaded, the game watched as
//! `acping_Tick` watches acs.exe, and the same script steps and log for the checks.
//!
//! Before the theme, the game's files are chosen ([`choose_game_files`]): Steam's install (or the
//! one remembered), a folder or a zip of the game, with extra cars and tracks laid over it
//! (`rustyac_content::imports`), checked against the shipped `base.json`. Drive checks the
//! picked cars and track (a DLC's list, or whether a mod loads) before it starts the game.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use webkit2gtk::{SecurityManagerExt, SettingsExt, WebContextExt as _, WebViewExt, WebsiteDataManagerExt};
use wry::http::{Request, Response};
use wry::{NewWindowResponse, PageLoadEvent, RequestAsyncResponder, WebContext, WebViewBuilder, WebViewBuilderExtUnix, WebViewExtUnix};

use rustyac_content::imports::{Layers, Remembered};
use rustyac_content::required::Lists;

use crate::bridge::{split_url, Answer, Bridge, Event};
use crate::files::Files;
use crate::folders::AppFolder;
use crate::ini::IniFile;
use crate::page::{current_theme, js_event, parse_step, Step, Theme, CONTROLS_PAGE, WIZARD_JS};
use crate::{appini, net, setup, sys};

/// How the launcher is started (as on Windows).
pub struct Options {
    pub ac_root: String,
    pub docs: String,
    pub game: PathBuf,
    pub game_args: Vec<String>,
    pub player_name: String,
    pub player_cfg: Option<PathBuf>,
    pub script: Option<Vec<String>>,
    pub log: Option<PathBuf>,
    pub offscreen: Option<(i32, i32)>,
    pub lobby: Option<String>,
}

/// The theme's `index.html` as a URL: a path off Windows starts with `/` already.
fn file_url(path: &str) -> String {
    net::uri_path(path).replacen("file:////", "file:///", 1)
}

/// Are the game's files laid over one another (a zip, extra cars and tracks)? Then WebKit cannot
/// read them as `file://`: the theme's pages come from `acfile://` ([`file_request`]).
static LAYERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The theme's page: `file://`, or `acfile://game/<path>` over laid files (a host: wry refuses
/// an empty one). The bridge's absolute paths in the page (`/…/skins/x/livery.png`) then resolve
/// to the same scheme.
fn theme_url(path: &str) -> String {
    let url = file_url(path);
    match LAYERED.load(std::sync::atomic::Ordering::Relaxed) {
        true => url.replacen("file:///", "acfile://game/", 1),
        false => url,
    }
}

/// An `acfile://game/<path>` request: the file through the launcher's files (the layers), as
/// `file://` reads the disk.
fn file_request(request: Request<Vec<u8>>, responder: RequestAsyncResponder) {
    // (`+` is a name's own: url_decode would make it a space)
    let path = crate::query::url_decode(&request.uri().path().replace('+', "%2B"));
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let mime = match ext.as_str() {
        "html" | "htm" => "text/html",
        "js" => "text/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ttf" | "otf" | "woff" | "woff2" => "font/ttf",
        "ogg" | "ogv" => "video/ogg",
        "mp3" => "audio/mpeg",
        "txt" | "ini" => "text/plain",
        _ => "application/octet-stream",
    };
    let response = match crate::files::with(|f| f.read(&path)) {
        Ok(bytes) => Response::builder().status(200).header("Content-Type", mime).body(bytes),
        Err(_) => Response::builder().status(404).body(Vec::new()),
    };
    responder.respond(response.expect("a response"));
}

/// Where a script is.
enum Running {
    Idle,
    WaitUntil(Instant),
    /// `asked`: when the page was last asked (wry drops the answer of a script sent before the
    /// first page has loaded, so a question unanswered for a second is asked again)
    Checking { deadline: Instant, asked: Instant, expr: String, answer: Arc<Mutex<Option<bool>>> },
    Busy(Rc<RefCell<bool>>),
    GameExit(Instant, bool),
}

struct App {
    window: gtk::Window,
    webview: wry::WebView,
    bridge: Bridge,
    theme: Theme,
    options: Options,
    game: Option<Child>,
    ac_active: bool,
    acticks: u32,
    pinging: bool,
    queue: VecDeque<String>,
    steps: VecDeque<Step>,
    running: Running,
    log: Option<std::fs::File>,
    exit_code: i32,
    /// The shipped lists (`required/` beside the program), for Drive's checks; `None`: not there.
    lists: Option<Lists>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

/// The shipped required-files lists: `required/` beside the program.
pub fn shipped_lists() -> Option<Lists> {
    let folder = std::env::current_exe().ok()?.parent()?.join("required");
    Lists::load(&folder).ok()
}

/// A message box that does not hold up the page (a script's run only logs it).
fn tell(window: Option<&gtk::Window>, text: &str) {
    let dialog = gtk::MessageDialog::new(window, gtk::DialogFlags::DESTROY_WITH_PARENT, gtk::MessageType::Info, gtk::ButtonsType::Ok, text);
    dialog.connect_response(|d, _| d.close());
    dialog.show();
}

impl App {
    fn log(&mut self, line: &str) {
        if let Some(file) = &mut self.log {
            let _ = writeln!(file, "{line}");
        }
    }

    fn exec(&self, script: &str) {
        let _ = self.webview.evaluate_script(script);
    }

    /// `ExecuteJS(script, name)`, as on Windows.
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
    fn load_ended(&mut self, url: &str) {
        self.log(&format!("loaded {url}"));
        let url = url.to_lowercase();
        if url.contains("file://") && url.contains("index.html") && self.theme.load_default_libs {
            self.include_default_libs();
        }
    }

    fn scheme_event(&mut self, host: &str, path: &str) {
        match host {
            "uiready" => {
                self.log("event uiready");
                if !self.pinging {
                    self.pinging = true;
                    glib::timeout_add_local(Duration::from_millis(2000), || {
                        with_app(App::ping);
                        glib::ControlFlow::Continue
                    });
                }
                self.enter_theme("ACIntroEvent");
            }
            "forcetotheme" => self.enter_theme("ACIntroEvent"),
            "setlanguage" => self.log(&format!("event setlanguage {path}")),
            _ => {}
        }
    }

    /// Drive's checks: the race's cars and track against the DLC lists, or whether they load.
    fn check_race(&mut self) -> Result<(), String> {
        let race = IniFile::new(appini::get(&self.bridge.folders, "race"));
        let read = |s: &str, k: &str| race.read(Some(s), Some(k));
        if read("REMOTE", "ACTIVE") == "1" || read("REPLAY", "ACTIVE") == "1" {
            return Ok(());
        }
        let Some(lists) = &self.lists else {
            self.log("check: no required-files lists beside the program, Drive is not checked");
            return Ok(());
        };
        let root = rustyac_content::install::ac_root().unwrap_or_default();
        let exists = |p: &str| rustyac_content::vfs::is_file(&root.join(p));
        let mut picks = vec![("tracks", read("RACE", "TRACK"), read("RACE", "CONFIG_TRACK"))];
        for n in 0..64 {
            // `-`: the player's car, [RACE] MODEL (as the game reads it)
            let car = match read(&format!("CAR_{n}"), "MODEL") {
                m if m == "-" => read("RACE", "MODEL"),
                m => m,
            };
            if !car.is_empty() && !picks.iter().any(|p| p.1.eq_ignore_ascii_case(&car)) {
                picks.push(("cars", car, String::new()));
            }
        }
        for (kind, id, layout) in picks.into_iter().filter(|p| !p.1.is_empty()) {
            if !lists.check_pick(kind, &id, &exists)? {
                rustyac_content::required::loads(&root, kind, &id, &layout, lists, &exists)?;
            }
        }
        Ok(())
    }

    fn event(&mut self, event: Event) {
        self.log(&format!("event {event:?}"));
        match event {
            Event::Start => {
                if self.game.as_mut().is_some_and(|g| matches!(g.try_wait(), Ok(None))) {
                    return;
                }
                if let Err(why) = self.check_race() {
                    self.log(&format!("check: {why}"));
                    if self.options.script.is_none() {
                        tell(Some(&self.window), &why);
                    }
                    return self.ping();
                }
                let mut cmd = crate::page::game_command(&self.bridge.folders, &self.options.game, &self.options.game_args);
                // the game reads the same files: a zip or extra roots through RUSTYAC_GAME_FILES
                // (inherited), else this folder
                if std::env::var_os("RUSTYAC_GAME_FILES").is_none() {
                    cmd.env("AC_ROOT", crate::page::os_path(&self.options.ac_root));
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
                let _ = self.webview.load_url(&theme_url(&self.theme.path));
            }
            Event::Exit => self.window.close(),
            Event::ContentErrors(list) => self.log(&format!("ERRORS with content: {list} view log for details")),
            Event::OpenControls => {
                let _ = self.webview.load_html(CONTROLS_PAGE);
            }
            Event::StartMusic | Event::StopMusic | Event::SetVolume => {}
        }
    }

    /// `acping_Tick`, watching rustyac instead of a process named acs.
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
                self.window.present();
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

    /// Runs the script on until a step has to wait (as on Windows).
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
            Running::Checking { deadline, asked, expr, answer } => {
                let a = *answer.lock().unwrap();
                match a {
                    Some(true) => self.log(&format!("ok until {expr}")),
                    _ if now >= deadline => self.fail(&format!("until {expr}")),
                    Some(false) => {
                        *answer.lock().unwrap() = None;
                        self.check(&expr, &answer);
                        return self.running = Running::Checking { deadline, asked: now, expr, answer };
                    }
                    None if now >= asked + Duration::from_secs(1) => {
                        self.check(&expr, &answer);
                        return self.running = Running::Checking { deadline, asked: now, expr, answer };
                    }
                    None => return self.running = Running::Checking { deadline, asked, expr, answer },
                }
            }
            Running::GameExit(deadline, started) => {
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
                self.window.close();
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
                let answer = Arc::new(Mutex::new(None));
                self.check(&expr, &answer);
                self.running = Running::Checking { deadline: now + Duration::from_millis(ms), asked: now, expr, answer };
            }
            Step::Shot(file) => {
                if let Err(e) = rustyac_content::guard::check(Path::new(&file)) {
                    return self.fail(&format!("shot {file}: {e}"));
                }
                let done = Rc::new(RefCell::new(false));
                let flag = done.clone();
                self.webview.webview().snapshot(webkit2gtk::SnapshotRegion::Visible, webkit2gtk::SnapshotOptions::NONE, None::<&gio::Cancellable>, move |surface| {
                    let saved = surface.ok().and_then(|s| {
                        let image = gtk::cairo::ImageSurface::try_from(s).ok()?;
                        gdk::pixbuf_get_from_surface(&image, 0, 0, image.width(), image.height())?.savev(&file, "png", &[]).ok()
                    });
                    if saved.is_none() {
                        with_app(|app| app.fail(&format!("shot {file}")));
                    }
                    *flag.borrow_mut() = true;
                });
                self.running = Running::Busy(done);
            }
            Step::GameExit(ms) => self.running = Running::GameExit(now + Duration::from_millis(ms), false),
            Step::Log(text) => self.log(&format!("log {text}")),
        }
    }

    fn check(&self, expr: &str, answer: &Arc<Mutex<Option<bool>>>) {
        let answer = answer.clone();
        let _ = self.webview.evaluate_script_with_callback(&format!("(function(){{try{{return !!({expr});}}catch(e){{return false;}}}})()"), move |result| {
            *answer.lock().unwrap() = Some(result == "true");
        });
    }
}

/// Script errors and console lines back to the shell, for the log: to the bridge's server
/// ([`xhr_js`]'s `__acPost`), as wry's `window.ipc` drops every message of a `file://` page (it
/// makes an `http::Request` of the page's address, and `http` refuses `file:///...`).
const REPORT_JS: &str = "(function(){var post=function(k,m){try{window.__acPost(k+' '+m)}catch(e){}};window.__acReport=post;\
window.addEventListener('error',function(e){post('error',e.message+' @ '+e.filename+':'+e.lineno)});\
window.addEventListener('unhandledrejection',function(e){post('error','unhandled rejection '+e.reason)});\
['log','warn','error'].forEach(function(l){var o=console[l];console[l]=function(){post('console.'+l,Array.prototype.map.call(arguments,String).join(' '));return o.apply(console,arguments)}});})();";

/// The page's requests to the bridge from its XMLHttpRequests: WebKitGTK (and WKWebView) give a
/// custom scheme no synchronous loads, and the theme makes many (`async: false`), so they go to
/// [`serve_bridge`]'s server on this computer instead. `http://ac/...` is CEF's `ac://` POST
/// (`OnBeforeResourceLoad`). Other `ac://` loads (a picture, say) stay on the scheme. The
/// synchronous ones go to a port of their own: WebKit counts its connections per host and port,
/// and ends a synchronous request after 10 s, so one must not wait behind the theme's ~2,500
/// asynchronous `getskinmeta` on a slow disk.
fn xhr_js([port, sync_port]: [u16; 2], token: &str) -> String {
    // a send that throws (a failed synchronous load) goes to the log with its URL
    format!("(function(){{var o=XMLHttpRequest.prototype.open,s=XMLHttpRequest.prototype.send,b='http://127.0.0.1:{port}/{token}/',bs='http://127.0.0.1:{sync_port}/{token}/';XMLHttpRequest.prototype.open=function(m,u,a){{if(typeof u=='string'){{if(u.indexOf('http://ac/')==0)u='ac://'+u.substring(10);if(u.indexOf('ac://')==0){{arguments[1]=(a===false?bs:b)+u.substring(5);this.__acUrl=u}}}}return o.apply(this,arguments)}};window.__acPost=function(m){{navigator.sendBeacon(bs+'__report',m)}};XMLHttpRequest.prototype.send=function(){{var u=this.__acUrl;if(u)this.addEventListener('loadend',function(){{if(this.status!==200&&window.__acReport)__acReport('error','XMLHttpRequest '+u+': status '+this.status)}});try{{return s.apply(this,arguments)}}catch(e){{window.__acReport&&__acReport('error','XMLHttpRequest '+this.__acUrl+': '+e.name+' '+e.message);throw e}}}}}})();")
}

/// A small HTTP server on 127.0.0.1 (a free port, a random token in every path) that hands each
/// request to the main thread's [`bridge_call`] and writes its answer back. Two ports
/// ([`xhr_js`]: the asynchronous requests' and the synchronous ones'), and the token.
fn serve_bridge() -> std::io::Result<([u16; 2], String)> {
    use std::io::{BufRead, BufReader, Read};
    let listeners = [std::net::TcpListener::bind(("127.0.0.1", 0))?, std::net::TcpListener::bind(("127.0.0.1", 0))?];
    let ports = [listeners[0].local_addr()?.port(), listeners[1].local_addr()?.port()];
    let token = glib::uuid_string_random().to_string();
    for (sync, listener) in [false, true].into_iter().zip(listeners) {
        let prefix = format!("/{token}/");
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let prefix = prefix.clone();
                std::thread::spawn(move || -> std::io::Result<()> {
                    stream.set_nodelay(true)?;
                    let mut reader = BufReader::new(stream.try_clone()?);
                    let mut line = String::new();
                    reader.read_line(&mut line)?;
                    let mut parts = line.split_whitespace();
                    let (method, target) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string());
                    let mut length = 0usize;
                    loop {
                        let mut header = String::new();
                        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
                            break;
                        }
                        if let Some((k, v)) = header.split_once(':') {
                            if k.trim().eq_ignore_ascii_case("content-length") {
                                length = v.trim().parse().unwrap_or(0);
                            }
                        }
                    }
                    let mut body = vec![0; length.min(64 << 20)];
                    reader.read_exact(&mut body)?;
                    let mut out = stream;
                    let cors = "Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: *\r\nCache-Control: no-store\r\nConnection: close\r\n";
                    let Some(rest) = target.strip_prefix(&prefix) else {
                        return out.write_all(format!("HTTP/1.1 403 Forbidden\r\n{cors}Content-Length: 0\r\n\r\n").as_bytes());
                    };
                    if rest == "__report" {
                        let line = String::from_utf8_lossy(&body).into_owned();
                        glib::idle_add_once(move || {
                            with_app(|app| app.log(&line));
                        });
                        return out.write_all(format!("HTTP/1.1 204 No Content\r\n{cors}Content-Length: 0\r\n\r\n").as_bytes());
                    }
                    if method == "OPTIONS" {
                        return out.write_all(format!("HTTP/1.1 204 No Content\r\n{cors}Content-Length: 0\r\n\r\n").as_bytes());
                    }
                    let url = format!("ac://{rest}");
                    let post = (method == "POST").then(|| String::from_utf8_lossy(&body).into_owned());
                    let (tx, rx) = std::sync::mpsc::channel::<Answer>();
                    let (asked, shown) = (std::time::Instant::now(), url.clone());
                    // on the main loop's thread, where the app is (invoke could run it on this one)
                    glib::idle_add_once(move || bridge_call(url, post, sync, Box::new(move |answer| drop(tx.send(answer)))));
                    let answer = rx.recv().unwrap_or_else(|_| Answer { mime: "text/plain".into(), ok: false, body: Vec::new() });
                    if asked.elapsed().as_secs() >= 5 {
                        // WebKit ends a synchronous request after 10 s: what kept one waiting
                        eprintln!("bridge: {shown} answered after {:.1} s", asked.elapsed().as_secs_f64());
                    }
                    // one write: headers and body apart would wait on the delayed ACK (Nagle)
                    let mut all = format!("HTTP/1.1 200 OK\r\n{cors}Content-Type: {}; charset=utf-8\r\nContent-Length: {}\r\n\r\n", answer.mime, answer.body.len()).into_bytes();
                    all.extend_from_slice(&answer.body);
                    out.write_all(&all)
                });
            }
        });
    }
    Ok((ports, token))
}

fn drain_events() {
    while let Some(Some(url)) = with_app(|app| app.queue.pop_front()) {
        let (host, path, _) = split_url(&url);
        with_app(|app| app.scheme_event(&host, &path));
    }
}

fn respond(responder: RequestAsyncResponder, answer: &Answer) {
    let response = Response::builder()
        .status(200)
        .header("Content-Type", format!("{}; charset=utf-8", answer.mime))
        .header("Access-Control-Allow-Origin", "*")
        .header("Cache-Control", "no-store")
        .body(answer.body.clone())
        .expect("a response");
    responder.respond(response);
}

/// An `ac://` request, answered through `reply` (the online ones from a thread of their own,
/// later). Its host is lower case, as Chromium (the launcher's CEF, WebView2) spells it.
/// `sync`: the page waits for it (the synchronous port of [`serve_bridge`]).
fn bridge_call(url: String, body: Option<String>, sync: bool, reply: Box<dyn FnOnce(Answer)>) {
    let url = match url.split_once("://") {
        Some((scheme, rest)) => {
            let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
            // an empty path is `/` there too (`ac://uimessage?msg=` is `ac://uimessage/?msg=`)
            let slash = if rest[end..].starts_with('/') { "" } else { "/" };
            format!("{scheme}://{}{slash}{}", rest[..end].to_ascii_lowercase(), &rest[end..])
        }
        None => url,
    };
    if let Some(Some(job)) = with_app(|app| app.bridge.job(&url, body.as_deref())) {
        with_app(|app| app.log(&format!("bridge {url} waits")));
        glib::MainContext::default().spawn_local(async move {
            let answer = gio::spawn_blocking(job).await.unwrap_or_else(|_| Answer { mime: "text/plain".into(), ok: false, body: Vec::new() });
            with_app(|app| {
                app.log(&format!("bridge {url} -> {} {} {}", answer.mime, answer.ok, answer.body.len()));
                app.queue.push_back(url);
            });
            reply(answer);
            glib::idle_add_local_once(drain_events);
        });
        return;
    }
    // a long listing off the main loop, so the page's synchronous requests meanwhile (WebKit ends
    // them after 10 s) are answered; a synchronous one is never queued behind it
    if let Some(Some(listing)) = with_app(|app| if sync { None } else { app.bridge.listing(&url, body.as_deref()) }) {
        let post = body.as_ref().map_or(String::new(), |b| format!(" POST {b}"));
        let id = LISTINGS.with(|l| {
            let mut l = l.borrow_mut();
            l.0 += 1;
            let id = l.0;
            l.1.insert(id, (url, post, reply));
            id
        });
        let _ = lister().send((id, listing));
        return;
    }
    let answered = with_app(|app| {
        let (answer, events) = app.bridge.request(&url, body.as_deref());
        let line = format!("bridge {url}{} -> {} {} {}", body.as_ref().map_or(String::new(), |b| format!(" POST {b}")), answer.mime, answer.ok, answer.body.len());
        app.log(&line);
        for name in app.bridge.not_ported.drain(..).collect::<Vec<_>>() {
            app.log(&format!("not ported: {name}"));
        }
        app.queue.push_back(url.clone());
        (answer, events)
    });
    let Some((answer, events)) = answered else { return };
    reply(answer);
    for event in events {
        with_app(|app| app.event(event));
    }
    glib::idle_add_local_once(drain_events);
}

type Waiting = (String, String, Box<dyn FnOnce(Answer)>);

thread_local! {
    /// The listings on [`lister`]'s thread: the last id, and each one's url, POST text and reply.
    static LISTINGS: RefCell<(u64, std::collections::HashMap<u64, Waiting>)> = RefCell::default();
}

/// The listings' thread ([`Bridge::listing`]): one at a time in the order asked, as Windows
/// answers them, so the log keeps their order.
fn lister() -> &'static std::sync::mpsc::Sender<(u64, crate::bridge::Listing)> {
    static LISTER: std::sync::OnceLock<std::sync::mpsc::Sender<(u64, crate::bridge::Listing)>> = std::sync::OnceLock::new();
    LISTER.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<(u64, crate::bridge::Listing)>();
        std::thread::spawn(move || {
            for (id, listing) in rx {
                let (answer, events) = listing();
                glib::idle_add_once(move || listed(id, answer, events));
            }
        });
        tx
    })
}

/// A listing's answer, on the main loop: logged as an answer in turn is.
fn listed(id: u64, answer: Answer, events: Vec<Event>) {
    let Some((url, post, reply)) = LISTINGS.with(|l| l.borrow_mut().1.remove(&id)) else { return };
    with_app(|app| {
        app.log(&format!("bridge {url}{post} -> {} {} {}", answer.mime, answer.ok, answer.body.len()));
        app.queue.push_back(url);
    });
    reply(answer);
    for event in events {
        with_app(|app| app.event(event));
    }
    glib::idle_add_local_once(drain_events);
}

fn answer_request(request: Request<Vec<u8>>, responder: RequestAsyncResponder) {
    let body = (request.method() == "POST").then(|| String::from_utf8_lossy(request.body()).into_owned());
    bridge_call(request.uri().to_string(), body, false, Box::new(move |answer| respond(responder, &answer)));
}

/// The launcher's files laid over the game's (a zip, extra cars and tracks): reads under the
/// game's folder go through the layers, everything else (the player's folder) is the disk.
struct LayeredFiles(Arc<Layers>);

impl LayeredFiles {
    fn under(&self, path: &str) -> Option<PathBuf> {
        let p = PathBuf::from(path.replace('\\', "/"));
        p.starts_with(&self.0.root).then_some(p)
    }
}

impl Files for LayeredFiles {
    fn read(&self, path: &str) -> std::io::Result<Vec<u8>> {
        use rustyac_content::vfs::Vfs;
        match self.under(path) {
            Some(p) => self.0.read(&p).map(|b| b.to_vec()),
            None => crate::files::Disk.read(path),
        }
    }
    fn write(&self, path: &str, bytes: &[u8]) -> std::io::Result<()> {
        crate::files::Disk.write(path, bytes)
    }
    fn is_file(&self, path: &str) -> bool {
        use rustyac_content::vfs::Vfs;
        self.under(path).map_or_else(|| crate::files::Disk.is_file(path), |p| self.0.is_file(&p))
    }
    fn is_dir(&self, path: &str) -> bool {
        use rustyac_content::vfs::Vfs;
        self.under(path).map_or_else(|| crate::files::Disk.is_dir(path), |p| self.0.is_dir(&p))
    }
    fn list(&self, folder: &str) -> std::io::Result<Vec<(String, bool)>> {
        use rustyac_content::vfs::Vfs;
        self.under(folder).map_or_else(|| crate::files::Disk.list(folder), |p| self.0.read_dir(&p))
    }
    fn create_dir_all(&self, path: &str) -> std::io::Result<()> {
        crate::files::Disk.create_dir_all(path)
    }
    fn rename(&self, from: &str, to: &str) -> std::io::Result<()> {
        crate::files::Disk.rename(from, to)
    }
    fn remove_file(&self, path: &str) -> std::io::Result<()> {
        crate::files::Disk.remove_file(path)
    }
    fn remove_dir_all(&self, path: &str) -> std::io::Result<()> {
        crate::files::Disk.remove_dir_all(path)
    }
    fn modified(&self, path: &str) -> Option<std::time::SystemTime> {
        // shortcut: a file inside a zip has no time here (zip times are not read); read them if the theme ever sorts the game's own files by date
        self.under(path).map_or_else(|| crate::files::Disk.modified(path), |_| None)
    }
    fn created(&self, path: &str) -> Option<std::time::SystemTime> {
        self.under(path).map_or_else(|| crate::files::Disk.created(path), |_| None)
    }
}

/// The game's files chosen and mounted for the launcher and the game it starts: the remembered
/// choice when it still opens and checks out (unless `choose`), else the chooser page, with
/// Steam's install (or the remembered one) offered and a **Change…** for another folder or a zip,
/// and extra cars and tracks to add. Returns AC's folder as the bridge sees it; `None`: the
/// player closed the window.
pub fn choose_game_files(choose: bool) -> Result<Option<PathBuf>, String> {
    // a tester's pack beside the program: its files, without asking or the base game's check
    if let Some(pack) = rustyac_content::imports::test_pack() {
        let layers = Layers::open(Some(&pack), &[])?;
        let line = format!("test pack: {} at {}", layers.ids("cars").join(", "), layers.ids("tracks").join(", "));
        eprintln!("rustyac-launcher: {line}");
        let _ = TEST_PACK_LINE.set(line);
        return Ok(Some(use_layers(layers, None)));
    }
    gtk::init().map_err(|e| format!("GTK: {e}"))?;
    let file = Remembered::file().ok_or("no HOME: rustyAC's settings folder is unknown")?;
    let lists = shipped_lists();
    let check = Rc::new(move |r: &Remembered| -> Result<Layers, String> {
        let layers = Layers::open(r.root.as_deref(), &r.extra)?;
        if let Some(lists) = &lists {
            lists.check_full(&|p| rustyac_content::vfs::Vfs::is_file(&layers, &layers.root.join(p)))?;
        }
        Ok(layers)
    });
    let mut choice = Remembered::load(&file);
    if !choose && choice.root.is_some() {
        if let Ok(layers) = check(&choice) {
            return Ok(Some(use_layers(layers, Some(&file))));
        }
    }
    if choice.root.is_none() {
        choice.root = rustyac_content::install::ac_root();
    }
    let state = Rc::new(RefCell::new((choice, None::<Layers>, false)));
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("rustyAC: Assetto Corsa's game files");
    window.set_default_size(760, 560);
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    window.add(&vbox);
    let page = |c: &Remembered, status: &Result<Layers, String>| chooser_page(c, status);
    let first = check(&state.borrow().0);
    let html = page(&state.borrow().0, &first);
    state.borrow_mut().1 = first.ok();
    let (st, win, check2) = (state.clone(), window.clone(), check.clone());
    let webview = WebViewBuilder::new()
        .with_html(html)
        .with_ipc_handler(move |request: Request<String>| {
            let what = request.body().clone();
            let (verb, arg) = what.split_once(' ').unwrap_or((&what, ""));
            let pick = |folder: bool, title: &str| -> Option<PathBuf> {
                let action = if folder { gtk::FileChooserAction::SelectFolder } else { gtk::FileChooserAction::Open };
                let dialog = gtk::FileChooserNative::new(Some(title), Some(&win), action, Some("Choose"), Some("Cancel"));
                if !folder {
                    let filter = gtk::FileFilter::new();
                    filter.add_pattern("*.zip");
                    filter.add_pattern("*.ZIP");
                    filter.set_name(Some("Zip files"));
                    dialog.add_filter(filter);
                }
                let path = (dialog.run() == gtk::ResponseType::Accept).then(|| dialog.filename()).flatten();
                dialog.destroy();
                path
            };
            let mut s = st.borrow_mut();
            match verb {
                "continue" if s.1.is_some() => {
                    s.2 = true;
                    win.close();
                    return;
                }
                "root-folder" | "root-zip" => {
                    if let Some(p) = pick(verb == "root-folder", "The folder (or zip) with Assetto Corsa's game files") {
                        s.0.root = Some(p);
                    }
                }
                "add-folder" | "add-zip" => {
                    if let Some(p) = pick(verb == "add-folder", "A folder or zip with cars and / or tracks") {
                        if !s.0.extra.contains(&p) {
                            s.0.extra.push(p);
                        }
                    }
                }
                "remove" => {
                    if let Ok(i) = arg.parse::<usize>() {
                        if i < s.0.extra.len() {
                            s.0.extra.remove(i);
                        }
                    }
                }
                _ => return,
            }
            let status = check2(&s.0);
            let html = chooser_page(&s.0, &status);
            s.1 = status.ok();
            drop(s);
            CHOOSER.with(|c| {
                if let Some(w) = c.borrow().as_ref() {
                    let _ = w.load_html(&html);
                }
            });
        })
        .build_gtk(&vbox)
        .map_err(|e| format!("the browser: {e}"))?;
    CHOOSER.with(|c| *c.borrow_mut() = Some(webview));
    window.connect_delete_event(|_, _| {
        gtk::main_quit();
        glib::Propagation::Proceed
    });
    window.show_all();
    gtk::main();
    CHOOSER.with(|c| c.borrow_mut().take());
    let (choice, layers, accepted) = Rc::try_unwrap(state).map_err(|_| "chooser still in use")?.into_inner();
    match (accepted, layers) {
        (true, Some(layers)) => {
            choice.save(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            Ok(Some(use_layers(layers, Some(&file))))
        }
        _ => Ok(None),
    }
}

thread_local! {
    static CHOOSER: RefCell<Option<wry::WebView>> = const { RefCell::new(None) };
}

/// "test pack: <car> at <track>" when a tester's pack is the game's files (the window's title).
static TEST_PACK_LINE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Mounts the choice for the launcher (its files) and names it for the game it starts (`file`,
/// the remembered choice; a tester's pack has none: the game finds the pack itself).
fn use_layers(layers: Layers, file: Option<&Path>) -> PathBuf {
    let root = layers.root.clone();
    rustyac_content::install::set_ac_root(Some(root.clone()));
    if !layers.is_plain() {
        // before any thread of the launcher starts; the game it starts inherits it
        if let Some(file) = file {
            std::env::set_var("RUSTYAC_GAME_FILES", file);
        }
        let shared = Arc::new(layers);
        LAYERED.store(true, std::sync::atomic::Ordering::Relaxed);
        rustyac_content::vfs::mount(Some(shared.clone()));
        crate::files::mount(Some(Arc::new(LayeredFiles(shared))));
    }
    root
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// rustyAC's own page for the choice (not the launcher's).
fn chooser_page(choice: &Remembered, status: &Result<Layers, String>) -> String {
    let (line, ok) = match status {
        Ok(layers) => (format!("Assetto Corsa found in <b>{}</b>. {} cars and {} tracks.", escape(&choice.root.as_ref().map(|p| p.display().to_string()).unwrap_or_default()), layers.ids("cars").len(), layers.ids("tracks").len()), true),
        Err(why) => (escape(why), false),
    };
    let extra: String = choice.extra.iter().enumerate().map(|(i, p)| format!("<li><span>{}</span><button onclick=\"post('remove {i}')\">Remove</button></li>", escape(&p.display().to_string()))).collect();
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>rustyAC: the game files</title><style>\
:root{{--bg:#16181c;--panel:#20242a;--text:#e8e8e8;--dim:#9aa0a8;--accent:#e0a030;--bad:#e06050}}\
body{{margin:0;background:var(--bg);color:var(--text);font:15px/1.45 system-ui,sans-serif}}main{{max-width:680px;margin:0 auto;padding:24px 16px}}\
h1{{font-size:22px;margin:0 0 16px}}section{{background:var(--panel);border-radius:8px;padding:16px;margin-bottom:16px}}h2{{font-size:17px;margin:0 0 10px}}\
p.status{{margin:0 0 12px;word-break:break-all}}p.bad{{color:var(--bad)}}p.note{{color:var(--dim);margin:0 0 10px}}ul{{list-style:none;padding:0;margin:0 0 10px}}\
li{{display:flex;gap:10px;align-items:center;justify-content:space-between;margin:6px 0;word-break:break-all}}\
button{{background:#2e343c;color:var(--text);border:1px solid #444b55;border-radius:6px;padding:6px 14px;font:inherit;cursor:pointer}}\
button:hover,button:focus-visible{{border-color:var(--accent)}}button.primary{{background:var(--accent);color:#111;border-color:var(--accent)}}button:disabled{{opacity:.4;cursor:default}}\
.bar{{display:flex;gap:10px;flex-wrap:wrap}}</style></head><body><main><h1>Assetto Corsa's game files</h1>\
<section><h2>The game</h2><p class=\"status{bad}\" role=\"status\">{line}</p><div class=\"bar\"><button onclick=\"post('root-folder')\">Change… (a folder)</button><button onclick=\"post('root-zip')\">Change… (a zip)</button></div></section>\
<section><h2>Extra cars and tracks</h2><p class=\"note\">Folders or zips with cars and / or tracks, read where they are, added to the game's own.</p><ul>{extra}</ul>\
<div class=\"bar\"><button onclick=\"post('add-folder')\">Add a folder…</button><button onclick=\"post('add-zip')\">Add a zip…</button></div></section>\
<div class=\"bar\"><button class=\"primary\" onclick=\"post('continue')\"{disabled}>Continue</button></div></main>\
<script>function post(m){{window.ipc.postMessage(m)}}</script></body></html>",
        bad = if ok { "" } else { " bad" },
        disabled = if ok { "" } else { " disabled" },
    )
}

/// Starts the launcher: the player's folder is set up, the window opens with the theme, and
/// the main loop runs until it is closed. Returns the exit code (1: a script step failed).
pub fn run(options: Options) -> Result<i32, String> {
    gtk::init().map_err(|e| format!("GTK: {e}"))?;
    let folders = AppFolder::new(&options.ac_root, &options.docs);
    struct Boxes;
    impl setup::Ask for Boxes {
        fn version_mismatch(&self, text: &str) -> bool {
            let d = gtk::MessageDialog::new(None::<&gtk::Window>, gtk::DialogFlags::MODAL, gtk::MessageType::Question, gtk::ButtonsType::OkCancel, text);
            d.set_title("Version Mismatch");
            let ok = d.run() == gtk::ResponseType::Ok;
            d.close();
            ok
        }
        fn setup_error(&self, text: &str) {
            let d = gtk::MessageDialog::new(None::<&gtk::Window>, gtk::DialogFlags::MODAL, gtk::MessageType::Error, gtk::ButtonsType::Ok, text);
            d.set_title("Error setting up user folders");
            d.run();
            d.close();
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

    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    match TEST_PACK_LINE.get() {
        Some(line) => window.set_title(&format!("Assetto Corsa Launcher ({line})")),
        None => window.set_title("Assetto Corsa Launcher"),
    }
    match options.offscreen {
        Some((w, h)) => {
            // a check's window: its size, never taking the focus (run under a virtual X server)
            window.set_default_size(w, h);
            window.set_accept_focus(false);
            window.set_focus_on_map(false);
        }
        None => {
            window.set_default_size(1600, 900);
            let full = launcher_ini.read(Some("WINDOW"), Some("fullscreen"));
            if full == "1" || full.is_empty() {
                window.maximize();
            }
        }
    }
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    window.add(&vbox);

    // the launcher's browser keeps its data in launcherdata (CefSettings.CachePath)
    let mut context = WebContext::new(Some(PathBuf::from(crate::page::os_path(&folders.user_launcher_data_dir))));
    let scripted = options.script.is_some();
    let (ports, token) = serve_bridge().map_err(|e| format!("the bridge's server: {e}"))?;
    let builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_initialization_script(REPORT_JS)
        .with_initialization_script(xhr_js(ports, &token))
        .with_initialization_script(WIZARD_JS)
        .with_asynchronous_custom_protocol("ac".into(), |_, request, responder| answer_request(request, responder))
        .with_asynchronous_custom_protocol("acfile".into(), |_, request, responder| file_request(request, responder))
        .with_ipc_handler(|request: Request<String>| {
            let text = request.body().clone();
            with_app(|app| app.log(&text));
        })
        .with_on_page_load_handler(|event, url| {
            if matches!(event, PageLoadEvent::Finished) {
                with_app(|app| app.load_ended(&url));
            }
        })
        // the theme opens no windows of its own
        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
        .with_autoplay(true);
    // a scripted run (a check) reaches this computer only: the theme's news and video feeds never leave it
    let webview = builder.build_gtk(&vbox).map_err(|e| format!("the browser: {e}"))?;
    {
        // CEF's settings: file pages may read files and call ac://
        let wk = webview.webview();
        if let Some(settings) = WebViewExt::settings(&wk) {
            settings.set_allow_file_access_from_file_urls(true);
            settings.set_allow_universal_access_from_file_urls(true);
            // the page's console on the launcher's own output too (its log has it through ipc)
            settings.set_enable_write_console_messages_to_stdout(true);
        }
        if let Some(context) = wk.context() {
            if let Some(security) = context.security_manager() {
                security.register_uri_scheme_as_cors_enabled("ac");
                // the theme's pages over laid files: what file:// pages may do (local storage,
                // reading the game's files, the bridge)
                security.register_uri_scheme_as_local("acfile");
                security.register_uri_scheme_as_secure("acfile");
                security.register_uri_scheme_as_cors_enabled("acfile");
            }
            // a scripted run (a check) reaches this computer only: the theme's news and video
            // feeds never leave it; the bridge's server on 127.0.0.1 is reached directly
            if scripted {
                if let Some(data) = context.website_data_manager() {
                    let mut proxy = webkit2gtk::NetworkProxySettings::new(Some("http://127.0.0.1:9"), &["localhost", "127.0.0.1", "::1"]);
                    data.set_network_proxy_settings(webkit2gtk::NetworkProxyMode::Custom, Some(&mut proxy));
                }
            }
        }
    }

    let log = options.log.as_ref().and_then(|p| rustyac_content::guard::create(p).ok());
    let steps: VecDeque<Step> = options.script.iter().flatten().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).filter_map(|l| parse_step(l.trim())).collect();
    let url = theme_url(&theme.path);
    let host = sys::Unix { player_name: options.player_name.clone() };
    let mut bridge = Bridge::new(folders, Box::new(host));
    bridge.lobby = Arc::new(Mutex::new(crate::online::Lobby::new(options.lobby.clone())));
    APP.with(|a| {
        *a.borrow_mut() = Some(App {
            window: window.clone(),
            webview,
            bridge,
            theme,
            options,
            game: None,
            ac_active: false,
            acticks: 0,
            pinging: false,
            queue: VecDeque::new(),
            steps,
            running: Running::Idle,
            log,
            exit_code: 0,
            lists: shipped_lists(),
        })
    });
    window.connect_delete_event(|_, _| {
        gtk::main_quit();
        glib::Propagation::Proceed
    });
    window.show_all();
    with_app(|app| {
        let _ = app.webview.load_url(&url);
    });
    if scripted {
        glib::timeout_add_local(Duration::from_millis(100), || {
            with_app(App::step);
            glib::ControlFlow::Continue
        });
    }
    gtk::main();
    let code = APP.with(|a| {
        let mut app = a.borrow_mut().take();
        let code = app.as_ref().map_or(0, |a| a.exit_code);
        if let Some(app) = &mut app {
            app.log("closed");
            if let Some(game) = &mut app.game {
                let _ = game.wait();
            }
        }
        code
    });
    drop(context);
    Ok(code)
}
