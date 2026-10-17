// SPDX-License-Identifier: GPL-3.0-or-later

//! AC's launcher in the page: the theme's `ac://` requests answered by the launcher's own
//! bridge (`crates/rustyac-launcher`), the one the desktop launcher runs, on the page's files.
//!
//! The game's folder is the page's source (the preview pack or the player's folder), read
//! through a small JavaScript object the page hands in (`base`: `kind`, `list`, `read`, `size`,
//! `modified`, all synchronous: the page has read what the bridge reads before the theme
//! starts). The player's folder (Documents\Assetto Corsa) is a [`MemFiles`] the page keeps in
//! its own storage with [`Launcher::snapshot`] and gives back to [`Launcher::new`].

use std::collections::HashMap;
use std::io::{Error, ErrorKind, Result};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use js_sys::{Array, Function, Reflect, Uint8Array};
use rustyac_launcher::bridge::{Bridge, Event, Host};
use rustyac_launcher::files::{self, Files, MemFiles};
use rustyac_launcher::folders::AppFolder;
use rustyac_launcher::{setup, transcript};
use wasm_bindgen::prelude::*;

/// Where the game's folder is for the bridge (a name: nothing is on a disk).
pub const AC_ROOT: &str = "C:\\assettocorsa";
/// Where the player's folder is for the bridge.
pub const DOCS_ROOT: &str = "C:\\Documents\\Assetto Corsa";

/// A path as the launcher writes it: `/` as `\`, no `\` doubled or at the end.
fn normal(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars().map(|c| if c == '/' { '\\' } else { c }) {
        if !(c == '\\' && out.ends_with('\\')) {
            out.push(c);
        }
    }
    out.trim_end_matches('\\').to_string()
}

/// The page's object over the game's folder.
struct Base(JsValue);

// wasm32-unknown-unknown runs this crate on one thread: the bridge's `Files` must be `Send +
// Sync` for the desktop's threads, and nothing here is ever shared with another thread.
unsafe impl Send for Base {}
unsafe impl Sync for Base {}

impl Base {
    fn call(&self, name: &str, path: &str) -> JsValue {
        let Ok(f) = Reflect::get(&self.0, &name.into()) else { return JsValue::UNDEFINED };
        let Some(f) = f.dyn_ref::<Function>() else { return JsValue::UNDEFINED };
        f.call1(&self.0, &path.into()).unwrap_or(JsValue::UNDEFINED)
    }
    fn kind(&self, path: &str) -> u32 {
        self.call("kind", path).as_f64().unwrap_or(0.0) as u32
    }
}

/// The game's folder through [`Base`] (read only), the player's in memory.
struct PageFiles {
    /// [`AC_ROOT`] upper case, as paths are compared.
    ac: String,
    base: Base,
    docs: MemFiles,
    /// The player's files' write times (ms since 1970), for the results page's date.
    times: Mutex<HashMap<String, f64>>,
}

impl PageFiles {
    fn new(base: JsValue) -> PageFiles {
        PageFiles { ac: AC_ROOT.to_uppercase(), base: Base(base), docs: MemFiles::new(), times: Mutex::new(HashMap::new()) }
    }

    /// The path inside the game's folder (`/` between names, `""` for the folder itself), or
    /// `None` for a path elsewhere.
    fn in_game(&self, path: &str) -> Option<String> {
        let n = normal(path);
        let up = n.to_uppercase();
        if up.len() != n.len() || !up.starts_with(&self.ac) {
            return None;
        }
        let rest = &n[self.ac.len()..];
        if rest.is_empty() {
            Some(String::new())
        } else {
            rest.strip_prefix('\\').map(|r| r.replace('\\', "/"))
        }
    }
}

fn read_only() -> Error {
    Error::from(ErrorKind::PermissionDenied)
}

/// A time in ms since 1970 with its fraction kept (the launcher sorts special events by their
/// folders' times, and two can share a millisecond).
fn time_of(ms: f64) -> SystemTime {
    UNIX_EPOCH + Duration::from_nanos((ms.max(0.0) * 1e6).round() as u64)
}

impl Files for PageFiles {
    fn read(&self, path: &str) -> Result<Vec<u8>> {
        match self.in_game(path) {
            Some(rel) => self.base.call("read", &rel).dyn_into::<Uint8Array>().map(|a| a.to_vec()).map_err(|_| Error::from(ErrorKind::NotFound)),
            None => self.docs.read(path),
        }
    }
    fn write(&self, path: &str, bytes: &[u8]) -> Result<()> {
        if self.in_game(path).is_some() {
            return Err(read_only());
        }
        self.docs.write(path, bytes)?;
        self.times.lock().unwrap().insert(normal(path).to_uppercase(), js_sys::Date::now());
        Ok(())
    }
    fn is_file(&self, path: &str) -> bool {
        match self.in_game(path) {
            Some(rel) => self.base.kind(&rel) == 1,
            None => self.docs.is_file(path),
        }
    }
    fn is_dir(&self, path: &str) -> bool {
        match self.in_game(path) {
            Some(rel) => self.base.kind(&rel) == 2,
            None => self.docs.is_dir(path),
        }
    }
    fn list(&self, folder: &str) -> Result<Vec<(String, bool)>> {
        let Some(rel) = self.in_game(folder) else { return self.docs.list(folder) };
        if self.base.kind(&rel) != 2 {
            return Err(Error::from(ErrorKind::NotFound));
        }
        let list: Array = self.base.call("list", &rel).dyn_into().unwrap_or_default();
        let mut out: Vec<(String, bool)> = list
            .iter()
            .filter_map(|e| {
                let e: Array = e.dyn_into().ok()?;
                Some((e.get(0).as_string()?, e.get(1).is_truthy()))
            })
            .collect();
        // NTFS's order: by the upper-case names
        out.sort_by_key(|(name, _)| name.to_uppercase());
        Ok(out)
    }
    fn create_dir_all(&self, path: &str) -> Result<()> {
        match self.in_game(path) {
            Some(rel) if self.base.kind(&rel) == 2 => Ok(()),
            Some(_) => Err(read_only()),
            None => self.docs.create_dir_all(path),
        }
    }
    fn rename(&self, from: &str, to: &str) -> Result<()> {
        if self.in_game(from).is_some() || self.in_game(to).is_some() {
            return Err(read_only());
        }
        self.docs.rename(from, to)?;
        let mut times = self.times.lock().unwrap();
        if let Some(t) = times.remove(&normal(from).to_uppercase()) {
            times.insert(normal(to).to_uppercase(), t);
        }
        Ok(())
    }
    fn remove_file(&self, path: &str) -> Result<()> {
        if self.in_game(path).is_some() {
            return Err(read_only());
        }
        self.docs.remove_file(path)
    }
    fn remove_dir_all(&self, path: &str) -> Result<()> {
        if self.in_game(path).is_some() {
            return Err(read_only());
        }
        self.docs.remove_dir_all(path)
    }
    fn modified(&self, path: &str) -> Option<SystemTime> {
        match self.in_game(path) {
            Some(rel) => self.base.call("modified", &rel).as_f64().filter(|ms| *ms >= 0.0).map(time_of),
            None => match self.times.lock().unwrap().get(&normal(path).to_uppercase()) {
                Some(ms) => Some(time_of(*ms)),
                None => self.docs.modified(path),
            },
        }
    }
    fn created(&self, path: &str) -> Option<SystemTime> {
        match self.in_game(path) {
            Some(_) => self.modified(path),
            None => self.docs.created(path),
        }
    }
    fn len(&self, path: &str) -> Option<u64> {
        match self.in_game(path) {
            Some(rel) => self.base.call("size", &rel).as_f64().filter(|n| *n >= 0.0).map(|n| n as u64),
            None => self.docs.read(path).ok().map(|b| b.len() as u64),
        }
    }
}

/// The page's answers to what the bridge cannot know: the screen, the player, the clock.
struct PageHost {
    /// `true`: transcript::Fixed's answers (a made-up screen, UTC), for the checks.
    fixed: bool,
    screen: (f64, f64),
    name: String,
    id: String,
}

fn get(o: &JsValue, key: &str) -> JsValue {
    Reflect::get(o, &key.into()).unwrap_or(JsValue::UNDEFINED)
}

impl Host for PageHost {
    fn primary_resolution(&self) -> (f64, f64) {
        if self.fixed {
            transcript::Fixed.primary_resolution()
        } else {
            self.screen
        }
    }
    fn display_modes(&self) -> Vec<(u32, u32)> {
        if self.fixed {
            transcript::Fixed.display_modes()
        } else {
            vec![(self.screen.0 as u32, self.screen.1 as u32)]
        }
    }
    fn video_modes(&self) -> Option<Vec<(i32, i32, f32)>> {
        if self.fixed {
            transcript::Fixed.video_modes()
        } else {
            Some(vec![(self.screen.0 as i32, self.screen.1 as i32, 60.0)])
        }
    }
    fn local_time(&self, file: Option<&str>) -> [i64; 7] {
        let ms = file.and_then(|f| files::with(|x| x.modified(f))).map_or_else(js_sys::Date::now, |t| t.duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64));
        let d = js_sys::Date::new(&JsValue::from_f64(ms));
        if self.fixed {
            [d.get_utc_full_year() as i64, d.get_utc_month() as i64 + 1, d.get_utc_date() as i64, d.get_utc_hours() as i64, d.get_utc_minutes() as i64, d.get_utc_seconds() as i64, d.get_utc_milliseconds() as i64]
        } else {
            [d.get_full_year() as i64, d.get_month() as i64 + 1, d.get_date() as i64, d.get_hours() as i64, d.get_minutes() as i64, d.get_seconds() as i64, d.get_milliseconds() as i64]
        }
    }
    fn month_abbrev(&self, month: u32) -> String {
        transcript::Fixed.month_abbrev(month)
    }
    fn player_name(&self) -> String {
        if self.fixed {
            transcript::Fixed.player_name()
        } else {
            self.name.clone()
        }
    }
    fn player_id(&self) -> String {
        if self.fixed {
            transcript::Fixed.player_id()
        } else {
            self.id.clone()
        }
    }
}

fn event_name(e: &Event) -> String {
    match e {
        Event::Start => "Start".into(),
        Event::Home => "Home".into(),
        Event::Exit => "Exit".into(),
        Event::OpenControls => "OpenControls".into(),
        Event::StartMusic => "StartMusic".into(),
        Event::StopMusic => "StopMusic".into(),
        Event::SetVolume => "SetVolume".into(),
        Event::ContentErrors(text) => format!("ContentErrors:{text}"),
    }
}

/// The player's folder as bytes: `D <path>` lines for folders, `F <path>\t<ms>\t<length>` lines
/// each followed by the file's bytes.
fn snapshot_of(files: &PageFiles) -> Vec<u8> {
    let mut out = Vec::new();
    for folder in files.docs.folders() {
        out.extend_from_slice(format!("D {folder}\n").as_bytes());
    }
    let times = files.times.lock().unwrap();
    for (path, bytes) in files.docs.files() {
        let ms = times.get(&normal(&path).to_uppercase()).copied().unwrap_or(0.0);
        out.extend_from_slice(format!("F {path}\t{ms}\t{}\n", bytes.len()).as_bytes());
        out.extend_from_slice(&bytes);
    }
    out
}

fn restore(files: &PageFiles, mut data: &[u8]) {
    while let Some(end) = data.iter().position(|b| *b == b'\n') {
        let line = String::from_utf8_lossy(&data[..end]).into_owned();
        data = &data[end + 1..];
        if let Some(folder) = line.strip_prefix("D ") {
            let _ = files.docs.create_dir_all(folder);
        } else if let Some(file) = line.strip_prefix("F ") {
            let mut parts = file.split('\t');
            let (Some(path), Some(ms), Some(len)) = (parts.next(), parts.next(), parts.next()) else { return };
            let len: usize = len.parse().unwrap_or(0).min(data.len());
            let _ = files.docs.write(path, &data[..len]);
            if let Ok(ms) = ms.parse::<f64>() {
                if ms > 0.0 {
                    files.times.lock().unwrap().insert(normal(path).to_uppercase(), ms);
                }
            }
            data = &data[len..];
        } else {
            return;
        }
    }
}

struct Ask;

impl setup::Ask for Ask {
    // a browser has no older launcher version to ask about: the cfg files are merged
    fn version_mismatch(&self, _: &str) -> bool {
        false
    }
    fn setup_error(&self, message: &str) {
        web_sys::console::warn_1(&format!("launcher setup: {message}").into());
    }
}

/// The launcher's bridge on the page's files.
#[wasm_bindgen]
pub struct Launcher {
    bridge: Bridge,
    files: Arc<PageFiles>,
}

#[wasm_bindgen]
impl Launcher {
    /// `base`: the page's object over the game's folder; `host`: `{name, id, width, height}`;
    /// `snapshot`: the player's folder as [`Launcher::snapshot`] gave it (empty: a new one).
    /// The player's folder is set up as the desktop launcher sets it up when it starts.
    #[wasm_bindgen(constructor)]
    pub fn new(base: JsValue, host: JsValue, snapshot: &[u8]) -> Launcher {
        let files = Arc::new(PageFiles::new(base));
        restore(&files, snapshot);
        files::mount(Some(files.clone()));
        let host = PageHost {
            fixed: false,
            screen: (get(&host, "width").as_f64().unwrap_or(1920.0), get(&host, "height").as_f64().unwrap_or(1080.0)),
            name: get(&host, "name").as_string().unwrap_or_else(|| "Player".into()),
            id: get(&host, "id").as_string().unwrap_or_default(),
        };
        let folders = AppFolder::new(AC_ROOT, DOCS_ROOT);
        let _ = setup::setup_user_directory(&folders, &Ask);
        Launcher { bridge: Bridge::new(folders, Box::new(host)), files }
    }

    /// One `ac://` request: `{mime, ok, body (bytes), events: [names]}`.
    pub fn request(&mut self, url: &str, post: Option<String>) -> JsValue {
        let (answer, events) = self.bridge.request(url, post.as_deref());
        let out = js_sys::Object::new();
        let _ = Reflect::set(&out, &"mime".into(), &answer.mime.into());
        let _ = Reflect::set(&out, &"ok".into(), &answer.ok.into());
        let _ = Reflect::set(&out, &"body".into(), &Uint8Array::from(&answer.body[..]));
        let names: Array = events.iter().map(|e| JsValue::from(event_name(e))).collect();
        let _ = Reflect::set(&out, &"events".into(), &names);
        out.into()
    }

    /// The player's folder, to keep (see [`Launcher::new`]).
    pub fn snapshot(&self) -> Vec<u8> {
        snapshot_of(&self.files)
    }

    /// A file of the player's folder (`cfg/race.ini`), or nothing.
    pub fn read_doc(&self, path: &str) -> Option<Vec<u8>> {
        self.files.read(&format!("{DOCS_ROOT}\\{path}")).ok()
    }

    /// Writes a file of the player's folder (`out/race_out.json`), its folders made.
    pub fn write_doc(&self, path: &str, bytes: &[u8]) -> bool {
        let full = normal(&format!("{DOCS_ROOT}\\{path}"));
        let folder = full.rfind('\\').map_or("", |i| &full[..i]);
        self.files.create_dir_all(folder).is_ok() && self.files.write(&full, bytes).is_ok()
    }

    /// The bridge's names of the two folders, as its answers spell them.
    pub fn roots() -> Array {
        [AC_ROOT, DOCS_ROOT].iter().map(|s| JsValue::from(*s)).collect()
    }
}

/// The check of Tasks 35 and 36's request lists in the browser: `requests` run through the
/// bridge with transcript::Fixed's answers on `base` and an empty player's folder. Returns
/// `[transcript bytes, player's folder snapshot]`.
#[wasm_bindgen]
pub fn launcher_transcript(base: JsValue, requests: &str) -> Array {
    let files = Arc::new(PageFiles::new(base));
    files::mount(Some(files.clone()));
    // the player's folder as the desktop check makes it: `<player>\Assetto Corsa`, there
    let _ = files.docs.create_dir_all(DOCS_ROOT);
    let host = PageHost { fixed: true, screen: (0.0, 0.0), name: String::new(), id: String::new() };
    let out = transcript::run_text(AC_ROOT, DOCS_ROOT, requests, None, Box::new(host));
    let snap = snapshot_of(&files);
    files::mount(None);
    [JsValue::from(Uint8Array::from(&out[..])), JsValue::from(Uint8Array::from(&snap[..]))].iter().cloned().collect()
}
