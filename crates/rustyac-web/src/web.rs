// SPDX-License-Identifier: GPL-3.0-or-later

//! What the page calls (`web/app.js`).
//!
//! The page owns everything a browser makes asynchronous: picking a folder, fetching and
//! caching files, the frame callback, the keys and the pad. It hands the files in here
//! ([`FileWriter`], [`fs_list`]), where they are mounted in place of a disk, asks what is
//! there ([`index`]) and which files a drive needs ([`files_wanted`]), and then makes a
//! [`Game`] and calls [`Game::frame`] from `requestAnimationFrame`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use rustyac_content::vfs::{self, MemFs};
use rustyac_game::input::bindings::key_name;
use rustyac_game::input::ini::ControlsIni;
use rustyac_game::input::pad::PadState;
use rustyac_game::input::{device_name, DEVICE_PAD};
use rustyac_game::view::CarView;
use wasm_bindgen::prelude::*;

use crate::content;
use crate::controls;

thread_local! {
    /// Task 28: the session list the next offline drive runs with the referee (`?sessions=`).
    static SESSIONS: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Task 28: the session list of the next [`Game::create`] (`p10,q5,r3`; empty: no referee).
#[wasm_bindgen]
pub fn set_sessions(list: String) {
    SESSIONS.with(|s| *s.borrow_mut() = Some(list));
}
use crate::gpu::{Gpu, OtherCar};
use crate::session::Session;

/// The folder the page's files sit under (a name only: nothing is on a disk).
const ROOT: &str = "ac";

fn files() -> &'static Arc<MemFs> {
    static FILES: OnceLock<Arc<MemFs>> = OnceLock::new();
    FILES.get_or_init(|| {
        let files = Arc::new(MemFs::new());
        vfs::mount(Some(files.clone()));
        rustyac_content::install::set_ac_root(Some(PathBuf::from(ROOT)));
        files
    })
}

fn mounted(path: &str) -> String {
    format!("{ROOT}/{}", path.trim_start_matches('/'))
}

fn relative(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    text.strip_prefix(&format!("{ROOT}/")).unwrap_or(&text).to_string()
}

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    files();
}

/// Task 37c's proof in the page (`window.rustyac.raceProof`): a drive of the launcher's files
/// with the inputs of a drive the desktop recorded, as `examples/race_proof` runs it. JSON:
/// the chained state hash, the hash of the list of every step's hash, the steps, the cars,
/// whether the page's setup is the recording's, and race_out.json.
#[wasm_bindgen]
pub fn race_proof(race: &str, assists: Option<String>, seed: u32, tape: &[u8]) -> Result<String, JsError> {
    files();
    let proof = crate::race::replay_race(race, assists.as_deref(), seed, tape).map_err(|e| JsError::new(&e))?;
    let steps: Vec<u8> = proof.step_hashes.iter().flat_map(|h| h.to_le_bytes()).collect();
    let fnv = |bytes: &[u8]| bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3));
    let json = |text: &str| format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t"));
    Ok(format!(
        "{{\"chain\":\"{:016x}\",\"steps_hash\":\"{:016x}\",\"steps\":{},\"cars\":{},\"same_setup\":{},\"race_out\":{}}}",
        proof.chain,
        fnv(&steps),
        proof.step_hashes.len(),
        proof.cars,
        proof.header == proof.tape_header,
        proof.race_out.map_or("null".to_string(), |bytes| json(&String::from_utf8_lossy(&bytes)))
    ))
}

/// Task 37c's speed test (`window.rustyac.raceSpeed`): the physics of a race.ini drive, the
/// player on the autodriver: `skip` steps (the grid's countdown), then `steps` steps timed, as
/// fast as they go, without drawing. JSON: the cars and the timed milliseconds.
#[wasm_bindgen]
pub fn race_speed(race: &str, assists: Option<String>, seed: u32, skip: u32, steps: u32) -> Result<String, JsError> {
    files();
    let mut session = crate::session::Session::from_race(race, assists.as_deref(), seed).map_err(|e| JsError::new(&e))?;
    session.input.lock().unwrap().autodrive = true;
    let mut run = |n: u32| -> Result<(), JsError> {
        let mut done = 0;
        while done < n {
            done += session.advance(0.09).map_err(|e| JsError::new(&e))?.steps;
        }
        Ok(())
    };
    run(skip)?;
    let started = now_ms();
    run(steps)?;
    let ms = now_ms() - started;
    Ok(format!("{{\"cars\":{},\"ms\":{ms:.1}}}", 1 + session.sim.opponents.len()))
}

/// The version of the build, for the page's footer.
#[wasm_bindgen]
pub fn version() -> String {
    format!("rustyAC {} (browser preview), maths: {:?}", env!("CARGO_PKG_VERSION"), rustyac_math::backend())
}

/// Says which files exist (paths relative to the game's folder, one per line) without
/// handing over their bytes.
#[wasm_bindgen]
pub fn fs_list(paths: &str) {
    for path in paths.lines().filter(|line| !line.is_empty()) {
        files().list(&mounted(path));
    }
}

/// Does the file have its bytes here already?
#[wasm_bindgen]
pub fn fs_has(path: &str) -> bool {
    files().has_bytes(&mounted(path))
}

/// Megabytes of file held in memory right now.
#[wasm_bindgen]
pub fn fs_megabytes() -> f64 {
    files().bytes_held() as f64 / 1e6
}

/// A small file in one piece.
#[wasm_bindgen]
pub fn fs_put(path: &str, bytes: &[u8]) {
    files().insert(&mounted(path), Arc::from(bytes));
}

/// A file that arrives in pieces (a 441 MB track model is streamed, never held twice).
#[wasm_bindgen]
pub struct FileWriter {
    bytes: Arc<[u8]>,
    filled: usize,
    hash: Option<crate::pack::Sha256>,
}

#[wasm_bindgen]
impl FileWriter {
    /// Room for a file of `size` bytes. `hashed`: work out the SHA-256 as the pieces arrive
    /// (a pack's manifest says what it has to be).
    #[wasm_bindgen(constructor)]
    pub fn new(size: f64, hashed: bool) -> Result<FileWriter, JsValue> {
        // (a wasm memory is 4 GB at most; the game's largest files are under half a gigabyte)
        if !(0.0..=3.0e9).contains(&size) {
            return Err(JsValue::from_str(&format!("a file of {size} bytes cannot be held in memory")));
        }
        Ok(FileWriter { bytes: std::iter::repeat_n(0u8, size as usize).collect(), filled: 0, hash: hashed.then(crate::pack::Sha256::new) })
    }

    /// The next piece. Returns the bytes received so far.
    pub fn write(&mut self, chunk: &[u8]) -> Result<f64, JsValue> {
        let bytes = Arc::get_mut(&mut self.bytes).ok_or("the file is already mounted")?;
        let end = self.filled + chunk.len();
        if end > bytes.len() {
            return Err(JsValue::from_str("more bytes than the file was said to have"));
        }
        bytes[self.filled..end].copy_from_slice(chunk);
        self.filled = end;
        if let Some(hash) = &mut self.hash {
            hash.update(chunk);
        }
        Ok(end as f64)
    }

    /// Mounts the file and returns its SHA-256 (empty when it was not asked for). An error
    /// when pieces are missing, or when `sha256` is given and is not the file's.
    pub fn commit(self, path: &str, sha256: &str) -> Result<String, JsValue> {
        if self.filled != self.bytes.len() {
            return Err(JsValue::from_str(&format!("{path}: {} of {} bytes arrived", self.filled, self.bytes.len())));
        }
        let hash = self.hash.map(crate::pack::Sha256::finish).unwrap_or_default();
        if !sha256.is_empty() && !hash.is_empty() && !hash.eq_ignore_ascii_case(sha256) {
            return Err(JsValue::from_str(&format!("{path}: the file is not the one the pack's manifest lists (SHA-256 {hash}, expected {sha256})")));
        }
        files().insert(&mounted(path), self.bytes);
        Ok(hash)
    }
}

pub(crate) fn json_text(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_refusal(refused: &Option<String>) -> String {
    match refused {
        // (without the mount's own folder name in front of a path)
        Some(why) => json_text(&why.replace(&format!("{ROOT}/"), "").replace(&format!("{ROOT}\\"), "")),
        None => "null".to_string(),
    }
}

/// The cars and tracks that are listed, as JSON: `{"cars": [{"id", "name", "refused"}],
/// "tracks": [{"track", "layout", "name", "refused"}]}`. `refused` is the loader's reason or
/// null. `check_cars`: build every car whose data is here to see whether it loads (a car
/// whose data has not been handed in yet is not refused, only unknown).
#[wasm_bindgen]
pub fn index(check_cars: bool) -> String {
    let root = Path::new(ROOT);
    let cars: Vec<String> = content::car_ids(root)
        .into_iter()
        .map(|id| {
            let data = format!("content/cars/{id}/data.acd");
            let here = fs_has(&data) || !files().is_listed(&mounted(&data));
            let refused = if check_cars && here { content::check_car(&id).err() } else { None };
            format!("{{\"id\": {}, \"name\": {}, \"refused\": {}}}", json_text(&id), json_text(&content::car_name(root, &id)), json_refusal(&refused))
        })
        .collect();
    let tracks: Vec<String> = content::tracks(root)
        .into_iter()
        .map(|t| format!("{{\"track\": {}, \"layout\": {}, \"name\": {}, \"refused\": {}}}", json_text(&t.track), json_text(&t.layout), json_text(&t.name), json_refusal(&t.refused)))
        .collect();
    format!("{{\"cars\": [{}], \"tracks\": [{}]}}", cars.join(", "), tracks.join(", "))
}

/// Does this car load? Empty, or the loader's refusal.
#[wasm_bindgen]
pub fn check_car(car: &str) -> String {
    let result = content::check_car(car).err().map(|why| why.replace(&format!("{ROOT}/"), "")).unwrap_or_default();
    // (the check asked for the car's collider model, which a list does not need: not wanted)
    files().take_wanted();
    result
}

/// The files a drive needs that are listed but not here yet, one per line. Call it again
/// after fetching them: with the first files in hand more can be named (the model a
/// `lods.ini` points to). Empty: everything is here.
#[wasm_bindgen]
pub fn files_wanted(car: &str, track: &str, layout: &str) -> String {
    let mut wanted: Vec<String> = content::files_of_drive(Path::new(ROOT), car, track, layout).iter().map(|path| relative(path)).filter(|path| !fs_has(path)).collect();
    // and what a load of this car or track stumbled over last time
    let mine = [format!("content/cars/{car}/").to_ascii_lowercase(), format!("content/tracks/{track}/").to_ascii_lowercase(), "system/".to_string()];
    wanted.extend(files().take_wanted().iter().map(|path| relative(Path::new(path))).filter(|path| !fs_has(path) && mine.iter().any(|prefix| path.to_ascii_lowercase().starts_with(prefix))));
    wanted.sort();
    wanted.dedup();
    wanted.join("\n")
}

/// Gives the memory of the big model files back once a drive is loaded. The car's collider
/// stays: it is a few kilobytes, and a new car (N) reads it again.
#[wasm_bindgen]
pub fn fs_forget_models() {
    for path in files().paths() {
        let lower = path.to_ascii_lowercase();
        if lower.ends_with(".kn5") && !lower.ends_with("/collider.kn5") {
            files().forget(&path);
        }
    }
}

/// The Controls panel's settings and where they came from; `None`: the built-in layout.
fn panel() -> &'static Mutex<Option<(ControlsIni, String)>> {
    static PANEL: OnceLock<Mutex<Option<(ControlsIni, String)>>> = OnceLock::new();
    PANEL.get_or_init(|| Mutex::new(None))
}

const BUILT_IN: &str = "the built-in layout";

fn panel_now() -> (ControlsIni, String) {
    panel().lock().unwrap().clone().unwrap_or_else(|| (controls::defaults(), BUILT_IN.to_string()))
}

/// Keeps the settings; ones that are the built-in layout's again are kept as "none".
fn panel_keep(ini: ControlsIni, origin: &str) -> String {
    let json = controls::to_json(&ini, origin);
    *panel().lock().unwrap() = (ini != controls::defaults()).then(|| (ini, origin.to_string()));
    json
}

/// The Controls panel as JSON (`crate::controls::to_json`): every value AC's pad and keyboard
/// classes read, with its range, its value now and the built-in one.
#[wasm_bindgen]
pub fn controls_json() -> String {
    let (ini, origin) = panel_now();
    controls::to_json(&ini, &origin)
}

/// The settings as the text of a `controls.ini`, for the page to keep; empty for the
/// built-in layout (nothing to keep).
#[wasm_bindgen]
pub fn controls_text() -> String {
    panel().lock().unwrap().as_ref().map(|(ini, _)| ini.to_text()).unwrap_or_default()
}

/// Takes a whole `controls.ini` (the player's own file, or what the page kept), read by the
/// desktop's parser. Returns the panel's JSON. A running [`Game`] takes the settings with
/// [`Game::controls_changed`].
#[wasm_bindgen]
pub fn controls_load(bytes: &[u8], origin: &str) -> Result<String, JsValue> {
    let ini = controls::from_file(bytes)?;
    Ok(panel_keep(ini, origin))
}

/// Changes one value of the panel. Returns the panel's JSON.
#[wasm_bindgen]
pub fn controls_set(section: &str, key: &str, value: &str) -> Result<String, JsValue> {
    let (mut ini, origin) = panel_now();
    controls::set(&mut ini, section, key, value)?;
    let origin = if origin.ends_with(", changed here") { origin } else { format!("{origin}, changed here") };
    Ok(panel_keep(ini, &origin))
}

/// Back to the built-in layout. Returns the panel's JSON.
#[wasm_bindgen]
pub fn controls_reset() -> String {
    panel_keep(controls::defaults(), BUILT_IN)
}

/// One drive: the simulation and its picture.
#[wasm_bindgen]
pub struct Game {
    session: Session,
    gpu: Gpu,
    last_ms: Option<f64>,
    view: CarView,
    notes: Vec<String>,
    /// Frames, steps and milliseconds since the page last asked.
    frames: u32,
    steps: u32,
    work_ms: f64,
    dropped_seconds: f64,
    drawn: crate::gpu::DrawStats,
    /// Online: the model on the card of every other player's car (the session's order).
    remote_models: Vec<Option<usize>>,
    /// Online: the WebSocket's end in here.
    link: Option<std::rc::Rc<crate::net::Link>>,
}

pub(crate) fn now_ms() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map_or(0.0, |p| p.now())
}

#[wasm_bindgen]
impl Game {
    /// Builds the car on the track and the picture on the canvas. The files have to be
    /// here ([`files_wanted`] empty). `spawn`: `pit`, `hotlap` or `start`. `backend`:
    /// `webgl` forces WebGL2. `texture_size`: 0 = what suits the device. `samples`: 4 or 1.
    #[allow(clippy::too_many_arguments)]
    pub async fn create(canvas: web_sys::HtmlCanvasElement, car: String, track: String, layout: String, spawn: String, auto_shifter: bool, backend: String, texture_size: u32, samples: u32) -> Result<Game, JsValue> {
        let gpu = Gpu::new(canvas, backend == "webgl", samples).await?;
        let sessions = SESSIONS.with(|s| s.borrow_mut().take());
        let session = Session::with_sessions(&car, &track, &layout, auto_shifter, &spawn, sessions.as_deref())?;
        Ok(Game::finish(gpu, session, texture_size, None))
    }

    /// Task 37b: the drive AC's launcher asked for: `race_ini` and `assists_ini` are the
    /// player's `cfg/race.ini` and `cfg/assists.ini` as the launcher wrote them; the car and
    /// the track are race.ini's (their files have to be here). `seed`: the game's `rand()`
    /// seed (the page gives the clock's milliseconds, as the game takes them).
    #[allow(clippy::too_many_arguments)]
    pub async fn create_race(canvas: web_sys::HtmlCanvasElement, race_ini: String, assists_ini: Option<String>, seed: u32, backend: String, texture_size: u32, samples: u32) -> Result<Game, JsValue> {
        let gpu = Gpu::new(canvas, backend == "webgl", samples).await?;
        let session = Session::from_race(&race_ini, assists_ini.as_deref(), seed)?;
        Ok(Game::finish(gpu, session, texture_size, None))
    }

    /// The drive of [`Game::create_race`] as `key=value` lines (car, skin, track, layout,
    /// spawn, session_type, sessions); empty for another drive.
    pub fn drive(&self) -> String {
        self.session.drive.clone()
    }

    /// The end of a drive with a session list: the referee's `race_out.json` (what the game
    /// writes to `out/` when it closes, and AC's launcher shows on its results page), or
    /// nothing without a referee.
    pub fn finish_race(&mut self) -> Option<Vec<u8>> {
        self.session.sim.finish_referee().map(|(json, _, _)| json)
    }

    /// The same on a server: `join` is a join that is done ([`crate::net::NetJoin`]); the
    /// car, the track and the other cars are the server's. The files have to be here
    /// (`NetJoin::status` lists what the server runs; [`files_wanted`] per car).
    /// `other_car_distance`: how far other cars are drawn with their model, metres (0: the
    /// default).
    #[allow(clippy::too_many_arguments)]
    pub async fn create_online(canvas: web_sys::HtmlCanvasElement, join: crate::net::NetJoin, spawn: String, auto_shifter: bool, backend: String, texture_size: u32, samples: u32, other_car_distance: f32) -> Result<Game, JsValue> {
        let mut gpu = Gpu::new(canvas, backend == "webgl", samples).await?;
        if other_car_distance > 0.0 {
            gpu.other_cars.model_m = other_car_distance;
        }
        // a device that has to unpack its textures is a small one: fewer car models at once
        if !gpu.compressed_textures {
            gpu.other_cars.models = 3;
            gpu.other_cars.model_m = gpu.other_cars.model_m.min(80.0);
        }
        let (online, link) = join.finish(Path::new(ROOT))?;
        let session = Session::new_online(online, auto_shifter, &spawn)?;
        Ok(Game::finish(gpu, session, texture_size, Some(link)))
    }

    fn finish(mut gpu: Gpu, mut session: Session, texture_size: u32, link: Option<std::rc::Rc<crate::net::Link>>) -> Game {
        // the Controls panel's settings, when they are not the built-in layout's
        if let Some((ini, origin)) = panel().lock().unwrap().as_ref() {
            session.set_controls(ini, origin);
        }
        let mut notes = vec![format!("picture: {}{}", gpu.backend, if gpu.compressed_textures { "" } else { ", no compressed textures (they are unpacked, at half the size)" })];
        // (without the loader's timing line: a browser's wasm has no clock of its own)
        notes.extend(session.race_notes.iter().cloned());
        notes.push(session.sim.track_summary.lines().filter(|line| !line.starts_with("loaded in")).collect::<Vec<_>>().join("\n"));
        let setup = &session.sim.setup;
        let (car_model, track_models) = content::model_files(Path::new(ROOT), &setup.car, &setup.track, &setup.layout);
        let options = gpu.model_options(texture_size);
        // a model that cannot be drawn is a line for the page, not the end of the drive
        match gpu.load_track(&track_models, &options) {
            Ok(summary) => notes.push(summary),
            Err(why) => notes.push(format!("the track's models are not drawn: {why}")),
        }
        match &car_model {
            Some(file) => match gpu.load_car(file, &options) {
                Ok(summary) => notes.push(summary),
                Err(why) => notes.push(format!("the car's model is not drawn (boxes instead): {why}")),
            },
            None => notes.push("the car has no model file here: boxes instead".to_string()),
        }
        // the other cars (race.ini's AI cars, then the other players'): one model per kind of
        // car (the driver's own kind is on the card already), with smaller textures: they are
        // seen from outside and afar
        let mut remote_models = Vec::new();
        let infos = session.sim.opponent_infos();
        if !infos.is_empty() {
            let mut loaded: Vec<(PathBuf, Option<usize>)> = car_model.iter().map(|file| (file.clone(), gpu.car_model())).collect();
            // (one budget for all of them: a server with ten kinds of car must not take ten
            // times the memory)
            let mut kinds: Vec<&str> = infos.iter().map(|info| info.name.as_str()).filter(|name| *name != setup.car).collect();
            kinds.sort_unstable();
            kinds.dedup();
            let other_options = crate::model::ModelOptions { texture_size: options.texture_size.min(512), texture_budget_mb: (192 / kinds.len().max(1) as u32).clamp(16, 96), ..options };
            for info in &infos {
                let model = match rustyac_game::sim::find_car_model(&info.name, &info.data_path) {
                    Some(file) => match loaded.iter().find(|(known, _)| *known == file) {
                        Some((_, model)) => *model,
                        None => {
                            let model = match gpu.load_car_model(&file, &other_options) {
                                Ok((model, summary)) => {
                                    notes.push(format!("{}: another {summary}", info.name));
                                    Some(model)
                                }
                                Err(why) => {
                                    notes.push(format!("the model of {} is not drawn (boxes instead): {why}", info.name));
                                    None
                                }
                            };
                            loaded.push((file, model));
                            model
                        }
                    },
                    None => None,
                };
                remote_models.push(model);
            }
            let limits = gpu.other_cars;
            notes.push(format!("other cars: {} ({} AI, {} on the server); drawn with their model up to {:.0} m ({} at once), as boxes up to {:.0} m", remote_models.len(), session.sim.opponents.len(), remote_models.len() - session.sim.opponents.len(), limits.model_m, limits.models, limits.box_m));
        }
        if let Some(wanted) = Some(files().take_wanted()).filter(|w| !w.is_empty()) {
            notes.push(format!("files that were asked for and are not here: {}", wanted.join(", ")));
        }
        let view = session.view();
        Game { session, gpu, last_ms: None, view, notes, frames: 0, steps: 0, work_ms: 0.0, dropped_seconds: 0.0, drawn: Default::default(), remote_models, link }
    }

    /// What loading found, one line each.
    pub fn notes(&self) -> String {
        self.notes.join("\n")
    }

    /// One frame: `now` is `requestAnimationFrame`'s time stamp, milliseconds.
    pub fn frame(&mut self, now: f64) -> Result<(), JsValue> {
        let seconds = self.last_ms.map_or(0.0, |last| (now - last) / 1000.0);
        self.last_ms = Some(now);
        let started = now_ms();
        let stats = self.session.advance(seconds)?;
        self.view = self.session.view();
        let camera = self.session.camera_frame(&self.view, seconds);
        let remote = self.session.remote_views();
        let others: Vec<OtherCar> = remote.iter().zip(&self.session.remote_shapes).zip(&self.remote_models).map(|((view, shape), model)| OtherCar { view, shape, model: *model }).collect();
        self.drawn = self.gpu.draw(&self.view, &self.session.shape, &camera, &others)?;
        self.frames += 1;
        self.steps += stats.steps;
        self.dropped_seconds += stats.dropped_seconds;
        self.work_ms += now_ms() - started;
        Ok(())
    }

    /// Runs physics steps without drawing or waiting (a check: the same number of steps
    /// gives the same state on every machine).
    pub fn run_steps(&mut self, steps: u32) -> Result<(), JsValue> {
        for _ in 0..steps {
            self.session.advance(rustyac_game::sim::DT as f64 * 1.000_001)?;
        }
        self.view = self.session.view();
        Ok(())
    }

    /// The car's whole state as one number (FNV-1a 64 of `save_state`), hex.
    pub fn state_hash(&self) -> String {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for word in self.session.sim.car.car.save_state() {
            for byte in word.to_le_bytes() {
                hash = (hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        format!("{hash:016x}")
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.gpu.resize(width, height);
    }

    /// A key went down or up: a Windows virtual-key code.
    pub fn key(&mut self, code: i32, down: bool) {
        let mut input = self.session.input.lock().unwrap();
        if down {
            input.keys.insert(code);
        } else {
            input.keys.remove(&code);
        }
    }

    /// The page lost the keyboard: nothing is held.
    pub fn release_keys(&mut self) {
        self.session.input.lock().unwrap().keys.clear();
    }

    /// The pad as XInput would give it: its buttons as `wButtons` bits, the triggers 0..255,
    /// the sticks -32768..32767 (up and right positive).
    #[allow(clippy::too_many_arguments)]
    pub fn pad(&mut self, connected: bool, buttons: u16, left_trigger: u8, right_trigger: u8, lx: i16, ly: i16, rx: i16, ry: i16) {
        self.session.input.lock().unwrap().pad = connected.then_some(PadState { buttons, left_trigger, right_trigger, thumb_lx: lx, thumb_ly: ly, thumb_rx: rx, thumb_ry: ry });
    }

    /// The pad's two motors, 0..1 (low frequency, high frequency).
    pub fn rumble(&self) -> Vec<f32> {
        let input = self.session.input.lock().unwrap();
        if input.pad.is_some() && self.view.device == DEVICE_PAD {
            input.rumble.to_vec()
        } else {
            vec![0.0, 0.0]
        }
    }

    /// One-off commands: bit 0 back to the pits (R), bit 8 back onto the track where the
    /// car is (Shift+R), bit 1 a new car (N), bit 6 the automatic gearbox on / off (G),
    /// bits 2 / 3 the traction control one level up / down (T, Shift+T), bits 4 / 5 the
    /// ABS (Y, Shift+Y). They run before the next physics step, as on the desktop.
    pub fn request(&mut self, events: u32) {
        self.session.request(events);
    }

    /// Clicks of the cockpit's brake-bias control: positive forward (]), negative rearward ([).
    pub fn bias(&mut self, clicks: i32) {
        self.session.request_bias(clicks);
    }

    /// F1: the next view (chase, chase 2, bonnet, bumper, dash, cockpit).
    pub fn next_camera(&mut self) {
        self.session.camera.f1();
    }

    pub fn set_autodrive(&mut self, on: bool) {
        self.session.input.lock().unwrap().autodrive = on;
    }

    /// What the display shows, as JSON.
    pub fn hud(&self) -> String {
        let v = &self.view;
        let lap = &v.lap;
        // (JSON has no NaN: a car with broken numbers shows zeros until it is rebuilt)
        let n = |x: f32| if x.is_finite() { x } else { 0.0 };
        format!(
            "{{\"kmh\": {:.1}, \"gear\": {}, \"rpm\": {:.0}, \"rpm_limit\": {:.0}, \"gas\": {:.3}, \"brake\": {:.3}, \"clutch\": {:.3}, \"steer\": {:.3}, \
             \"lap_ms\": {}, \"last_ms\": {}, \"best_ms\": {}, \"laps\": {}, \"valid\": {}, \"position\": {:.4}, \"in_pit_lane\": {}, \
             \"tc\": {}, \"abs\": {}, \"drs\": {}, \"auto_shifter\": {}, \"device\": {}, \"camera\": {}, \"seconds\": {:.2}, \"fuel\": {:.1}, \
             \"tc_text\": {}, \"abs_text\": {}, \"bias_text\": {}, \"tc_level\": {}, \"tc_levels\": {}, \"tc_fitted\": {}, \
             \"abs_level\": {}, \"abs_levels\": {}, \"abs_fitted\": {}, \"brake_bias\": {:.6}, \"note\": {}, \"online\": {}, \"referee\": {}}}",
            n(v.speed_kmh),
            v.gear,
            n(v.rpm),
            n(v.rpm_limit),
            n(v.gas),
            n(v.brake),
            n(v.clutch),
            n(v.steer),
            lap.current_ms,
            lap.last_ms,
            lap.best_ms,
            lap.laps,
            lap.valid,
            n(lap.position),
            lap.in_pit_lane,
            v.tc_in_action,
            v.abs_in_action,
            v.drs,
            v.auto_shifter,
            json_text(device_name(v.device)),
            json_text(&self.session.camera.name()),
            if v.drive_seconds.is_finite() { v.drive_seconds } else { 0.0 },
            n(v.fuel),
            // the aids' levels and the brake bias, worded by the code the desktop's display uses
            json_text(&v.tc_text()),
            json_text(&v.abs_text()),
            json_text(&v.bias_text()),
            v.tc_mode.0,
            v.tc_mode.1,
            v.tc_present,
            v.abs_mode.0,
            v.abs_mode.1,
            v.abs_present,
            n(v.brake_bias),
            json_text(v.aid_note().unwrap_or("")),
            self.online_json(),
            self.referee_json()
        )
    }

    /// Task 28: what the referee says, as JSON (`null` without one): the lines at the top
    /// (session, banner, position and laps, flags), the messages in the middle, the results.
    fn referee_json(&self) -> String {
        let list = |lines: &[String]| lines.iter().map(|line| json_text(line)).collect::<Vec<_>>().join(", ");
        if let Some(session) = &self.session.sim.online {
            return format!("{{\"top\": [{}], \"centre\": [], \"results\": [], \"fade\": 0}}", list(&session.referee_lines()));
        }
        let Some(hud) = self.session.sim.referee_hud() else {
            return "null".to_string();
        };
        let mut top = vec![if hud.banner.is_empty() { hud.session.clone() } else { format!("{}   {}", hud.session, hud.banner) }];
        top.extend(hud.lines.iter().cloned());
        top.extend(hud.flags.iter().cloned());
        if !hud.caption.is_empty() {
            top.push(hud.caption.clone());
        }
        // Task 28b: the pit menu while it is up, under the referee's lines
        top.extend(hud.pit.iter().cloned());
        format!("{{\"top\": [{}], \"centre\": [{}], \"results\": [{}], \"fade\": {:.3}}}", list(&top), list(&hud.centre), list(&hud.results), hud.fade)
    }

    /// Online: the server, the session, the drivers and the chat, as JSON (`null` offline).
    fn online_json(&self) -> String {
        let Some(session) = &self.session.sim.online else {
            return "null".to_string();
        };
        let client = &session.net.client;
        let drivers: Vec<String> = session
            .roster
            .iter()
            .filter(|entry| entry.connected)
            .map(|entry| {
                format!(
                    "{{\"slot\": {}, \"name\": {}, \"car\": {}, \"me\": {}, \"laps\": {}, \"best_ms\": {}, \"last_ms\": {}, \"ping\": {}}}",
                    entry.slot,
                    json_text(&entry.driver),
                    json_text(&entry.model),
                    entry.me,
                    entry.laps,
                    entry.best_lap_ms,
                    entry.last_lap_ms,
                    entry.ping
                )
            })
            .collect();
        let chat: Vec<String> = session.chat.iter().map(|line| json_text(line)).collect();
        let there = session.net.cars.iter().filter(|car| !car.is_disconnected).count();
        format!(
            "{{\"server\": {}, \"session\": {}, \"ping\": {}, \"slot\": {}, \"associated\": {}, \"lost\": {}, \"drivers\": [{}], \"chat\": [{}], \"cars_there\": {}, \"cars_drawn\": {}, \"cars_as_boxes\": {},              \"datagrams_in\": {}, \"datagrams_out\": {}, \"messages_in\": {}, \"datagrams_dropped\": {}}}",
            json_text(&client.accepted.server_name),
            json_text(&client.current_session.name),
            client.ping,
            client.session_id,
            client.is_associated,
            client.is_lost(),
            drivers.join(", "),
            chat.join(", "),
            there,
            self.drawn.other_models,
            self.drawn.other_boxes,
            client.datagrams_in,
            client.datagrams_out,
            client.messages_in,
            self.link.as_ref().map_or(0, |link| link.dropped())
        )
    }

    /// Online: says `text` in the chat. False when it did not go out (the game sends one
    /// line a second at most; nothing offline).
    pub fn chat(&mut self, text: &str) -> bool {
        let now = self.session.sim.sim_seconds() * 1000.0;
        match &mut self.session.sim.online {
            // (the game's own command: the list is shown here, nothing is sent)
            Some(session) if text.trim() == "/client_list" => {
                let lines = session.hud_lines();
                self.session.console.extend(lines.iter().cloned());
                session.chat.extend(lines.into_iter().skip(1).take(4));
                while session.chat.len() > 6 {
                    session.chat.pop_front();
                }
                true
            }
            Some(session) => session.net.client.send_chat(text, now),
            None => false,
        }
    }

    /// Online: tells the server the player is leaving (`ACClient::shutdown`). The page
    /// closes the WebSocket after it.
    pub fn leave(&mut self) {
        if let Some(session) = &mut self.session.sim.online {
            if !session.net.client.is_lost() {
                session.net.client.shutdown();
            }
        }
    }

    /// Online: what the session printed since the last call (who joined, laps, chat ...),
    /// one line each.
    pub fn take_console(&mut self) -> String {
        std::mem::take(&mut self.session.console).join("\n")
    }

    /// Online: where the other cars are, as JSON (a check reads it): per car its slot,
    /// whether it is there, its position and speed, and the distance from the driver's car.
    pub fn other_cars(&self) -> String {
        let Some(session) = &self.session.sim.online else {
            return "[]".to_string();
        };
        let me = self.view.body[3];
        let cars: Vec<String> = session
            .net
            .cars
            .iter()
            // (the views list race.ini's AI cars first)
            .zip(self.session.remote_views().into_iter().skip(self.session.sim.opponents.len()))
            .zip(&session.remote)
            .map(|((car, view), remote)| {
                let p = view.body[3];
                let d = [p[0] - me[0], p[1] - me[1], p[2] - me[2]];
                let n = |x: f32| if x.is_finite() { x } else { 0.0 };
                format!(
                    "{{\"slot\": {}, \"model\": {}, \"there\": {}, \"x\": {:.2}, \"y\": {:.2}, \"z\": {:.2}, \"kmh\": {:.1}, \"distance\": {:.1}}}",
                    car.session_id,
                    json_text(&remote.entry.model),
                    !car.is_disconnected,
                    n(p[0]),
                    n(p[1]),
                    n(p[2]),
                    n(car.state.speed * 3.6),
                    n((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt())
                )
            })
            .collect();
        format!("[{}]", cars.join(", "))
    }

    /// The counters since the last call, as JSON: frames, physics steps, the milliseconds a
    /// frame's physics and drawing took, the seconds slow frames dropped, what the last frame
    /// drew.
    pub fn take_stats(&mut self) -> String {
        let out = format!(
            "{{\"frames\": {}, \"steps\": {}, \"work_ms_per_frame\": {:.3}, \"meshes\": {}, \"triangles\": {}, \"total_steps\": {}, \"dropped_seconds\": {:.3}, \"file_megabytes\": {:.1}, \"backend\": {}, \"other_cars_drawn\": {}, \"other_cars_as_boxes\": {}}}",
            self.frames,
            self.steps,
            if self.frames > 0 { self.work_ms / self.frames as f64 } else { 0.0 },
            self.drawn.meshes,
            self.drawn.triangles,
            self.session.total_steps,
            self.dropped_seconds,
            fs_megabytes(),
            json_text(&self.gpu.backend),
            self.drawn.other_models,
            self.drawn.other_boxes
        );
        self.frames = 0;
        self.steps = 0;
        self.work_ms = 0.0;
        self.dropped_seconds = 0.0;
        out
    }

    /// The Controls panel changed: AC's keyboard and pad classes are built again from its
    /// settings before the next physics step. Nothing else about the drive changes.
    pub fn controls_changed(&mut self) {
        let (ini, origin) = panel_now();
        self.session.set_controls(&ini, &origin);
    }

    /// The keys the layout uses, as Windows virtual-key codes: with the page's own keys they
    /// are the only ones the page keeps from the browser.
    pub fn keys_used(&self) -> Vec<i32> {
        self.session.keys_used.clone()
    }

    /// The keys and pad buttons of the built-in layout, for the page's key list.
    pub fn bindings(&self) -> String {
        self.session.keys.clone()
    }
}

/// A key's name as the desktop game prints it.
#[wasm_bindgen]
pub fn name_of_key(code: i32) -> String {
    key_name(code)
}
