// SPDX-License-Identifier: GPL-3.0-or-later

//! What the page calls (`web/app.js`).
//!
//! The page owns everything a browser makes asynchronous: picking a folder, fetching and
//! caching files, the frame callback, the keys and the pad. It hands the files in here
//! ([`FileWriter`], [`fs_list`]), where they are mounted in place of a disk, asks what is
//! there ([`index`]) and which files a drive needs ([`files_wanted`]), and then makes a
//! [`Game`] and calls [`Game::frame`] from `requestAnimationFrame`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use rustyac_content::vfs::{self, MemFs};
use rustyac_game::input::bindings::key_name;
use rustyac_game::input::pad::PadState;
use rustyac_game::input::{device_name, DEVICE_PAD};
use rustyac_game::view::CarView;
use wasm_bindgen::prelude::*;

use crate::content;
use crate::gpu::Gpu;
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
    files().bytes_held() as f64 / 1_048_576.0
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
}

#[wasm_bindgen]
impl FileWriter {
    /// Room for a file of `size` bytes.
    #[wasm_bindgen(constructor)]
    pub fn new(size: f64) -> FileWriter {
        FileWriter { bytes: std::iter::repeat_n(0u8, size as usize).collect(), filled: 0 }
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
        Ok(end as f64)
    }

    /// Mounts the file. An error when pieces are missing.
    pub fn commit(self, path: &str) -> Result<(), JsValue> {
        if self.filled != self.bytes.len() {
            return Err(JsValue::from_str(&format!("{path}: {} of {} bytes arrived", self.filled, self.bytes.len())));
        }
        files().insert(&mounted(path), self.bytes);
        Ok(())
    }
}

fn json_text(text: &str) -> String {
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
    content::check_car(car).err().map(|why| why.replace(&format!("{ROOT}/"), "")).unwrap_or_default()
}

/// The files a drive needs that are listed but not here yet, one per line. Call it again
/// after fetching them: with the first files in hand more can be named (the model a
/// `lods.ini` points to). Empty: everything is here.
#[wasm_bindgen]
pub fn files_wanted(car: &str, track: &str, layout: &str) -> String {
    let mut wanted: Vec<String> = content::files_of_drive(Path::new(ROOT), car, track, layout).iter().map(|path| relative(path)).filter(|path| !fs_has(path)).collect();
    // and what a load stumbled over last time
    wanted.extend(files().take_wanted().iter().map(|path| relative(Path::new(path))).filter(|path| !fs_has(path)));
    wanted.sort();
    wanted.dedup();
    wanted.join("\n")
}

/// Gives the memory of the big model files back once a drive is loaded.
#[wasm_bindgen]
pub fn fs_forget_models() {
    for path in files().paths() {
        if path.to_ascii_lowercase().ends_with(".kn5") {
            files().forget(&path);
        }
    }
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
    drawn: crate::gpu::DrawStats,
}

fn now_ms() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map_or(0.0, |p| p.now())
}

#[wasm_bindgen]
impl Game {
    /// Builds the car on the track and the picture on the canvas. The files have to be
    /// here ([`files_wanted`] empty). `spawn`: `pit`, `hotlap` or `start`. `backend`:
    /// `webgl` forces WebGL2. `texture_size`: 0 = what suits the device. `samples`: 4 or 1.
    #[allow(clippy::too_many_arguments)]
    pub async fn create(canvas: web_sys::HtmlCanvasElement, car: String, track: String, layout: String, spawn: String, auto_shifter: bool, backend: String, texture_size: u32, samples: u32) -> Result<Game, JsValue> {
        let mut gpu = Gpu::new(canvas, backend == "webgl", samples).await?;
        let mut notes = vec![format!("picture: {}{}", gpu.backend, if gpu.compressed_textures { "" } else { ", no compressed textures (they are unpacked, at half the size)" })];
        let session = Session::new(&car, &track, &layout, auto_shifter, &spawn)?;
        notes.push(session.sim.track_summary.clone());
        let (car_model, track_models) = content::model_files(Path::new(ROOT), &car, &track, &layout);
        let options = gpu.model_options(texture_size);
        // a model that cannot be drawn is a line for the page, not the end of the drive
        match gpu.load_track(&track_models, &options) {
            Ok(summary) => notes.push(summary),
            Err(why) => notes.push(format!("the track's models are not drawn: {why}")),
        }
        match car_model {
            Some(file) => match gpu.load_car(&file, &options) {
                Ok(summary) => notes.push(summary),
                Err(why) => notes.push(format!("the car's model is not drawn (boxes instead): {why}")),
            },
            None => notes.push("the car has no model file here: boxes instead".to_string()),
        }
        if let Some(wanted) = Some(files().take_wanted()).filter(|w| !w.is_empty()) {
            notes.push(format!("files that were asked for and are not here: {}", wanted.join(", ")));
        }
        let view = session.view();
        Ok(Game { session, gpu, last_ms: None, view, notes, frames: 0, steps: 0, work_ms: 0.0, drawn: Default::default() })
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
        self.drawn = self.gpu.draw(&self.view, &self.session.shape, &camera)?;
        self.frames += 1;
        self.steps += stats.steps;
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
    /// car is (Shift+R), bit 1 a new car (N), bit 6 the automatic gearbox on / off (G).
    pub fn request(&mut self, events: u32) {
        self.session.request(events);
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
        format!(
            "{{\"kmh\": {:.1}, \"gear\": {}, \"rpm\": {:.0}, \"rpm_limit\": {:.0}, \"gas\": {:.3}, \"brake\": {:.3}, \"clutch\": {:.3}, \"steer\": {:.3}, \
             \"lap_ms\": {}, \"last_ms\": {}, \"best_ms\": {}, \"laps\": {}, \"valid\": {}, \"position\": {:.4}, \"in_pit_lane\": {}, \
             \"tc\": {}, \"abs\": {}, \"drs\": {}, \"auto_shifter\": {}, \"device\": {}, \"camera\": {}, \"seconds\": {:.2}, \"fuel\": {:.1}}}",
            v.speed_kmh,
            v.gear,
            v.rpm,
            v.rpm_limit,
            v.gas,
            v.brake,
            v.clutch,
            v.steer,
            lap.current_ms,
            lap.last_ms,
            lap.best_ms,
            lap.laps,
            lap.valid,
            lap.position,
            lap.in_pit_lane,
            v.tc_in_action,
            v.abs_in_action,
            v.drs,
            v.auto_shifter,
            json_text(device_name(v.device)),
            json_text(&self.session.camera.name()),
            v.drive_seconds,
            v.fuel
        )
    }

    /// The counters since the last call, as JSON: frames, physics steps, the milliseconds a
    /// frame's physics and drawing took, what the last frame drew.
    pub fn take_stats(&mut self) -> String {
        let out = format!(
            "{{\"frames\": {}, \"steps\": {}, \"work_ms_per_frame\": {:.3}, \"meshes\": {}, \"triangles\": {}, \"total_steps\": {}, \"dropped_seconds\": {:.3}, \"file_megabytes\": {:.1}, \"backend\": {}}}",
            self.frames,
            self.steps,
            if self.frames > 0 { self.work_ms / self.frames as f64 } else { 0.0 },
            self.drawn.meshes,
            self.drawn.triangles,
            self.session.total_steps,
            self.session.total_dropped_seconds,
            fs_megabytes(),
            json_text(&self.gpu.backend)
        );
        self.frames = 0;
        self.steps = 0;
        self.work_ms = 0.0;
        out
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
