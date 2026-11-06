// SPDX-License-Identifier: GPL-3.0-or-later

//! Full mode: AC's own renderer (`rustyac_game::render::ac`, kgl on wgpu) drawing the page's
//! game, and its fixed frame: the proof that the page asks kgl for what the desktop asks.
//!
//! The fixed frame: a drive as `Session::new` makes it (no physics step), the renderer made for
//! it (with AC's GUI and a `--gui-do` script when asked), then `frames` frames each 1/60 s after
//! the last. The kgl call log runs
//! from the renderer's making to the last frame. The page (`Game::full_proof`) and the native
//! `full_frame` example (kgl on wgpu with `--features wgpu`, on Direct3D 11 without) run the
//! same code.

use std::path::{Path, PathBuf};

use rustyac_game::render::ac::{AcOptions, AcRenderer};
use rustyac_game::render::gui::GuiSetup;
use rustyac_game::render::hud::HudInfo;
use rustyac_physics::data::ini::IniReader;
use rustyac_game::view::CarView;

use crate::session::Session;

/// A frame's time in the fixed frame.
pub const FIXED_SECONDS: f32 = 1.0 / 60.0;

/// AC's renderer with the session's track and cars, as rustyac.exe makes it from the same
/// race.ini (its sun, weather, groove and race start; without one, midday and clear). `warp`: Direct3D's or wgpu's WARP natively (the page has its
/// own device); `log`: the kgl call log's names (kgl on wgpu gives them out only then);
/// `video`: a video.ini read as `--video-ini` with `--video-ini-exact` reads it (the page's
/// graphics settings), or none: the game's defaults. `gui`: AC's GUI and HUD, made as
/// rustyac.exe makes them once the cars are loaded.
#[allow(clippy::too_many_arguments)]
pub fn renderer(root: &Path, width: u32, height: u32, session: &Session, warp: bool, log: bool, video: Option<&Path>, gui: Option<GuiSetup>, notes: &mut Vec<String>) -> Result<Box<AcRenderer>, String> {
    let sim = &session.sim;
    let info = sim.car_info();
    let others = sim.opponent_infos();
    // (Direct3D 11's gpu_log is its own command log below kgl: not wanted here)
    let wgpu = cfg!(any(target_arch = "wasm32", feature = "wgpu"));
    let look = &session.look;
    let options = AcOptions {
        warp,
        gpu_log: (log && wgpu).then(PathBuf::new),
        game: root.to_path_buf(),
        sun_angle: look.sun_angle,
        weather: look.weather.clone(),
        skin: None,
        video_exact: video.is_some(),
        cube_faces: None,
        mirror_size: None,
        sun_animation: look.sun_animation,
        groove: look.groove,
        mirror_hq: None,
        world_detail: None,
        render_seed: info.render_seed,
        cube_size: None,
        ideal_line: false,
        session_type: look.session_type.unwrap_or(if sim.setup.spawn == "hotlap" { 4 } else { 1 }),
        start_lights: look.start_lights(),
        video_ini: video.map(Path::to_path_buf),
        preset: None,
        aa_samples: None,
        post: None,
        fxaa: None,
        motion_blur: None,
        hdr_levels: false,
        own_dir: None,
    };
    let mut renderer = Box::new(AcRenderer::new(width, height, options)?);
    notes.extend(renderer.notes.iter().cloned());
    if let Some(folder) = &info.track_folder {
        notes.push(renderer.load_track(folder, &info.track_layout)?);
    }
    renderer.set_cars_to_be_loaded(1 + others.len());
    match renderer.load_car_info(&info, info.skin.clone())? {
        Some(summary) => {
            notes.push(summary);
            for (number, other) in others.iter().enumerate() {
                match renderer.load_car_info(other, other.skin.clone())? {
                    Some(summary) => notes.push(summary),
                    None => return Err(format!("no 3D model of {} (car {})", other.name, number + 1)),
                }
            }
            let crew = renderer.init_pit_crew();
            if !crew.is_empty() {
                notes.push(crew);
            }
        }
        None => notes.push(format!("no 3D model of {}: no car is drawn", info.name)),
    }
    renderer.finish_loading()?;
    if let Some(mut setup) = gui {
        // the graphics page's Rendering Mode: Triple Screen draws three views on the one canvas
        setup.triple_screens = renderer.triple_screens();
        if let Err(why) = renderer.enable_gui(setup) {
            notes.push(format!("AC's GUI could not be made: {why}"));
        }
    }
    Ok(renderer)
}

/// AC's apps the page cannot run, without an icon in the apps bar and never shown: the sound's
/// levels (`volumes`: the page's sound does not take them), force feedback (`ffb`: a browser
/// has none), the driver's eyes (`eyes`: the page's cameras do not take them), the frame
/// statistics (`gamestats`, `rstats`: only Direct3D 11 counts them) and the apps fed from the
/// physics thread (`telemetry`, `setupdebug`). AC's Python apps (apps/python) are not made in
/// the page: they need the player's python33.dll (rustyac.exe runs them, Task 42).
pub const PAGE_HIDDEN_APPS: &[&str] = &["volumes", "ffb", "eyes", "gamestats", "rstats", "telemetry", "setupdebug"];

/// Task 47d: the files every app writes when the game shuts down (`GameScreen::shutdown`), at the
/// paths rustyac.exe writes them, kept instead of written (the page saves them in its Documents
/// when a drive ends). Once: the apps are shut down.
pub fn shutdown_files(gui: &mut rustyac_game::render::gui::GameGui) -> Vec<(PathBuf, Vec<u8>)> {
    gui.apps.kept = Some(Vec::new());
    gui.shutdown_apps();
    gui.apps.kept.take().unwrap_or_default()
}

/// What AC's GUI reads of the drive and where its files are, as rustyac.exe's `make_ac` gives
/// it: `cfg_dir` is rustyAC's own cfg folder (what the apps write; the page keeps them, `shutdown_files`).
pub fn gui_setup(root: &Path, cfg_dir: &Path, session: &Session) -> GuiSetup {
    let sim = &session.sim;
    let info = sim.car_info();
    let others = sim.opponent_infos();
    let cars: Vec<&rustyac_game::sim::CarInfo> = std::iter::once(&info).chain(&others).collect();
    let car_ini = |car: &rustyac_game::sim::CarInfo| IniReader::load(&car.data_path.join("car.ini")).ok();
    let float = |car: &rustyac_game::sim::CarInfo, section: &str, key: &str| car_ini(car).and_then(|ini| ini.get_float(section, key).ok()).unwrap_or(0.0);
    let folders: Vec<String> = cars.iter().map(|car| car.name.clone()).collect();
    let manager = sim.referee.as_ref().map(|referee| &referee.manager);
    let documents_cfg = rustyac_content::install::documents().map(|folder| folder.join("cfg"));
    let own = cfg_dir.parent().unwrap_or(cfg_dir);
    GuiSetup {
        game: root.to_path_buf(),
        cfg_dir: cfg_dir.to_path_buf(),
        pit_compounds: info.pit_compounds.clone(),
        cars: rustyac_game::render::gui::car_names(root, None, &folders),
        steer_locks: cars.iter().map(|car| float(car, "CONTROLS", "STEER_LOCK")).collect(),
        fuel_light_min_liters: float(&info, "GRAPHICS", "FUEL_LIGHT_MIN_LITERS"),
        session_type: session.look.session_type.unwrap_or(if sim.setup.spawn == "hotlap" { 4 } else { 1 }),
        penalties: manager.is_some_and(|m| m.penalties_enabled),
        session_count: manager.map_or(1, |m| m.sessions.len().max(1) as u32),
        online: sim.online.is_some(),
        // (the page records no AC replay)
        replay: false,
        player_data: info.data_path.clone(),
        // (as the cars are drawn: the skin asked for, else the first of the folder)
        skins: cars
            .iter()
            .map(|car| {
                car.skin.clone().unwrap_or_else(|| {
                    let mut skins: Vec<String> = rustyac_content::vfs::read_dir(&root.join("content/cars").join(&car.name).join("skins")).unwrap_or_default().into_iter().filter(|(_, dir)| *dir).map(|(name, _)| name).collect();
                    skins.sort();
                    skins.first().cloned().unwrap_or_default()
                })
            })
            .collect(),
        // CarAvatar::initCommon 0x1400d56e0: car.ini [INFO] SHORT_NAME, else the screen name
        short_names: cars
            .iter()
            .map(|car| {
                let ini = car_ini(car);
                let key = |name: &str| ini.as_ref().filter(|ini| ini.has_key("INFO", name)).map(|ini| ini.get_string("INFO", name)).filter(|text| !text.is_empty());
                key("SHORT_NAME").or_else(|| key("SCREEN_NAME")).unwrap_or_else(|| car.name.clone())
            })
            .collect(),
        setup_snapshot: info.setup.as_deref().cloned(),
        pit_page: info.pit_page.as_deref().cloned(),
        telemetry_car: info.telemetry,
        car_physics: info.car_physics,
        engineer_masses: info.engineer_masses,
        wings: info.wings.clone(),
        suspension: info.suspension,
        setups_dir: own.join("setups"),
        documents_setups: documents_cfg.as_ref().and_then(|cfg| cfg.parent()).map(|folder| folder.join("setups")),
        track_name: info.track_folder.as_ref().and_then(|folder| folder.file_name()).map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
        track_layout: info.track_layout.clone(),
        out_dir: own.join("out"),
        esc_start: sim.setup.esc_start,
        // (set from the renderer)
        triple_screens: None,
        vr: false,
        open_vr: None,
        ai_drive: false,
        fixed_setup: manager.is_some_and(|m| m.fixed_setup),
        data_paths: cars.iter().map(|car| car.data_path.clone()).collect(),
        dev_apps: rustyac_game::render::gui::dev_apps_enabled(root, false),
        hidden_apps: PAGE_HIDDEN_APPS,
        language: rustyac_gui::locale::language(documents_cfg.as_deref(), None),
        // (the comparative telemetry's recorder is the desktop's)
        telemetry_folder: None,
        max_gear: rustyac_game::input::ini::ControlsIni::load_car_data(&info.data_path.join("drivetrain.ini")).map_or(0, |ini| ini.get_int("GEARS", "COUNT")),
        documents_cfg,
        // (the page runs no Python apps)
        // (no Python apps in the page: nothing reads them)
        python_cars: Vec::new(),
        server: None,
    }
}

/// What AC's GUI is told of the frame: the referee's state, the pause.
pub fn hud_info(session: &Session, paused: bool) -> HudInfo {
    // (rustyAC's own text display off, as rustyac.exe has it without --debug-hud)
    HudInfo { paused, hidden: true, referee: session.sim.referee_hud(), ..Default::default() }
}

/// What the page keeps of AC's GUI from frame to frame, as rustyac.exe's main loop keeps it.
#[derive(Default)]
pub struct GuiLoop {
    /// The pause menu's pause (`Sim::setPauseMode`): no physics steps.
    pub paused: bool,
    /// TAB is held (the table of TAB shows while it is).
    pub tab_held: bool,
    /// The ESC menu's Exit or the console's `exit`: the page leaves the drive.
    pub exit: bool,
    pits_locked: bool,
    /// Task 42b: AC's start camera while a session that began in the ESC menu waits for "Drive"
    pub start_camera: Option<rustyac_game::render::scene::StartCamera>,
    /// Task 42c: F7's free camera, the last frame's camera (where it starts), the mouse over
    /// the picture (x, y, right button held) and F4's fade back to the player's car
    pub free_camera: Option<rustyac_game::render::scene::MouseCamera>,
    pub last_frame: Option<rustyac_game::render::scene::CameraFrame>,
    pub mouse: (i32, i32, bool),
    pub focus: Option<rustyac_game::focus::Focus>,
    /// Task 46: F3's track cameras, and the name of the one the last frame showed
    pub track_cameras: rustyac_game::render::track_camera::TrackCameras,
    pub track_camera: String,
    /// F2's random camera (Full mode)
    pub random_camera: Option<rustyac_game::render::track_camera::RandomCamera>,
    /// Task 46b: F5's camera, F7's cinematic one (`video_editing.ini [CINEMATIC_CAMERAS]
    /// ACTIVE`), the scene camera they move while one shows, and the left and middle mouse
    /// buttons (AC's 1 and 4)
    pub onboard_free: Option<rustyac_game::render::onboard_free::OnboardFreeCamera>,
    pub cinematic: Option<rustyac_game::render::onboard_free::CinematicCamera>,
    pub shot: Option<rustyac_game::render::onboard_free::ShotCamera>,
    pub buttons: [bool; 2],
    /// (the setup editor's car: its jobs applied, and the 64-step slice it was taken in)
    published: Option<(usize, u64)>,
    generation: u64,
}

impl GuiLoop {
    /// After a frame: AC's GUI's asks to the drive, and the car back to the setup editor, as
    /// rustyac.exe's main loop and physics thread pass them.
    pub fn exchange(&mut self, renderer: &mut AcRenderer, session: &mut Session) {
        let Some(gui) = renderer.gui.as_mut() else { return };
        gui.tab_poll(self.tab_held);
        let sim = &mut session.sim;
        // online: what the client heard (chat, votes, the race's end) to AC's GUI, and the
        // GUI's chat lines and votes to the client, as rustyac.exe's loops pass them
        if let Some(online) = &mut sim.online {
            gui.online_heard(std::mem::take(&mut online.gui_events));
            for request in gui.take_online_requests() {
                match request {
                    rustyac_game::online::OnlineRequest::Chat { text, now } => {
                        online.net.client.send_chat(&text, now);
                    }
                    rustyac_game::online::OnlineRequest::Vote { kind, yes, guid } => online.send_vote(kind, yes, guid),
                }
            }
        }
        // (the physics thread's publish_setup: after its jobs, and a few times a second)
        let now = (sim.setup_log.len(), sim.steps / 64);
        if self.published != Some(now) {
            self.published = Some(now);
            self.generation += 1;
        }
        let mut snapshot = rustyac_gui::setup_car::SetupSnapshot::of(&sim.car.car, &sim.setup_car_name());
        snapshot.applied_jobs = sim.setup_log.len() as u64;
        sim.setup_jobs.extend(gui.setup_exchange(&(self.generation, Some(snapshot))));
        if let Some(menu) = gui.pit_page_exchange() {
            sim.apply_pit_page(&menu);
        }
        for click in gui.take_pit_clicks() {
            sim.pit_click(click);
        }
        use rustyac_game::input_file::event;
        use rustyac_gui::menus::SimAction;
        self.exit |= std::mem::take(&mut gui.exit_requested);
        for action in gui.take_actions() {
            match action {
                SimAction::SetPauseMode(paused) => self.paused = paused,
                SimAction::SkipCurrentSession => session.request(event::NEXT_SESSION),
                SimAction::RestartCurrentSession => session.request(event::RESTART_SESSION),
                SimAction::LockControlsUntilTime(_) | SimAction::GoToSpawnPosition(_) => session.request(event::BACK_TO_PITS),
                // ESCMenu::render: the controls locked while the pits' menu shows
                SimAction::SetControlsLock(true) => {
                    if gui.esc_menu_visible() && (!self.pits_locked || gui.pits_lock_stale) {
                        self.pits_locked = true;
                        gui.pits_lock_stale = false;
                        session.request(event::PITS_LOCK);
                    }
                }
                SimAction::StartRace(_) => {
                    self.pits_locked = false;
                    session.request(event::PITS_DRIVE);
                }
                SimAction::LoadPreviousSessionUserCamera => {
                    self.start_camera = None;
                    // (the page writes no files: the last camera is the one its Documents give)
                    if let Some(mode) = session.camera.load_previous() {
                        self.set_mode(session, mode);
                    }
                }
                SimAction::SetDrivableCamera(view) => session.camera.set_drivable(view),
                SimAction::SetCameraModeKept(mode) => {
                    (self.start_camera, self.free_camera) = (None, None);
                    session.camera.set_mode_kept(mode);
                }
                SimAction::SetTrackCameraSet(set) => self.track_cameras.set_current_set(set.max(0) as usize),
                // the results' on-board free camera and back, the ESC menu's slow turn
                SimAction::SetCameraMode(mode) => self.set_mode(session, mode),
                SimAction::SetIntroMode(on) => {
                    if let Some(f5) = self.onboard_free.as_mut() {
                        f5.esc_mode = on;
                    }
                }
                SimAction::Shutdown => self.exit = true,
                // (the page has no AC replay and no other car to watch; the rest are the
                // sound's, the chat's and the referee's)
                _ => {}
            }
        }
    }

    /// A key down (a Windows virtual-key code), as rustyac.exe's window gives AC's GUI its keys:
    /// Home the console, TAB the table, ESC the pause menu, the command modifier with the keys
    /// of controls.ini (`ctrl`) the apps, desktops, names and damage, and the race's restart.
    /// True when it was one of those.
    pub fn key(&mut self, renderer: &mut AcRenderer, session: &mut Session, code: u32, ctrl: bool, shift: bool) -> bool {
        let Some(gui) = renderer.gui.as_mut() else { return false };
        if code == 0x24 && !(ctrl && shift) && session.sim.online.is_none() {
            gui.console_toggle();
        }
        gui.key_down(code);
        let key = code as i32;
        let bindings = {
            let input = session.input.lock().unwrap();
            rustyac_game::input::bindings::Bindings::from_ini(input.controls.as_ref().unwrap_or(&rustyac_game::input::bindings::default_ini()), String::new())
        };
        match key {
            0x09 => {
                self.tab_held = true;
                gui.tab_down();
            }
            0x1b if !ctrl => {
                let moving = session.sim.online.is_some() && session.sim.car.car.speed * 3.6 > 1.0;
                gui.esc_key(moving);
            }
            _ if ctrl && key == bindings.key_reset_race => {
                if !gui.pause_menu_visible() {
                    session.request(rustyac_game::input_file::event::RESTART_SESSION);
                }
            }
            _ if ctrl && !self.paused && key == bindings.key_hide_apps => gui.hide_show_apps(),
            _ if ctrl && !self.paused && key == bindings.key_cycle_desktop => {
                gui.cycle_desktop();
            }
            _ if ctrl && !self.paused && key == bindings.key_driver_names => {
                gui.toggle_driver_names();
            }
            // Sim::onKeyDown, Ctrl+Numpad +/-: the cockpit's (and the dash view's) field of
            // view, half a degree a press, up to 125 and down to 1 (as rustyac.exe)
            0x6b | 0x6d if ctrl && !shift && self.start_camera.is_none() && self.free_camera.is_none() && session.camera.on_board() => {
                let fov = session.shape.onboard_fov;
                let new = if key == 0x6b { (fov < 125.0).then_some(fov + 0.5) } else { (fov > 1.0).then_some(fov - 0.5) };
                if let Some(new) = new {
                    session.shape.onboard_fov = new;
                    session.remote_shapes.iter_mut().for_each(|shape| shape.onboard_fov = new);
                    gui.add_message(&format!("FOV: {new:.1}"), "On Board Camera");
                }
            }
            _ if ctrl && !self.paused && key == bindings.key_show_damage => gui.show_damage(),
            _ if ctrl && !self.paused && key == bindings.key_hide_damage => {
                gui.toggle_damage();
            }
            _ => return code == 0x24,
        }
        true
    }
}

impl GuiLoop {
    /// `ACCameraManager::setMode(mode, false, false)`, as rustyac.exe.
    pub fn set_mode(&mut self, session: &mut Session, mode: i32) {
        let gui = self;
        rustyac_game::render::onboard_free::set_mode(mode, &mut session.camera, &mut gui.start_camera, &mut gui.free_camera, &mut gui.shot, gui.last_frame.as_ref(), session.shape.car_cameras.len(), gui.cinematic.is_some(), &mut gui.track_cameras);
    }

    /// What F5's and the cinematic camera read: the held keys and mouse buttons, the mouse.
    fn orbit_input(&self, session: &Session) -> rustyac_game::render::onboard_free::OrbitInput {
        let mut input = rustyac_game::render::onboard_free::OrbitInput::default();
        for code in &session.input.lock().unwrap().keys {
            input.keys[*code as usize & 0xff] = true;
        }
        // (either Shift, either Ctrl: GetAsyncKeyState(VK_SHIFT) is held for both)
        input.keys[0x10] |= input.keys[0xa0] | input.keys[0xa1];
        input.keys[0x11] |= input.keys[0xa2] | input.keys[0xa3];
        let (x, y, right) = self.mouse;
        [input.keys[0x01], input.keys[0x02], input.keys[0x04]] = [self.buttons[0], right, self.buttons[1]];
        // (a page cannot move the mouse: the cinematic look's wrap at the desktop's edges is kept
        // out of reach, far from them; the page locks the pointer while the look's button is held,
        // so the look goes on as past the desktop's wrap)
        input.cursor = (x + 1_000_000, y + 1_000_000);
        input.screen = (i32::MAX, i32::MAX);
        input
    }
}

/// One frame of the page's game: the player's car first, then the others.
pub fn draw(renderer: &mut AcRenderer, session: &mut Session, view: &CarView, seconds: f32, hud: &HudInfo, gui: &mut GuiLoop) {
    let dt = seconds.clamp(0.0, crate::session::MAX_FRAME_SECONDS as f32);
    let mut track_look = None;
    // ACCameraManager::update: the random camera's next draw (on the cockpit shake's rand())
    if let Some(random) = gui.random_camera.as_mut() {
        let mut rand = session.camera.cockpit.rand;
        random.update(dt, &mut session.camera, session.shape.car_cameras.len(), &mut gui.track_cameras, &mut rand);
        session.camera.cockpit.rand = rand;
    }
    let shown = session.camera.shown.filter(|_| gui.start_camera.is_none() && gui.free_camera.is_none());
    let camera = match (&mut gui.start_camera, &mut gui.free_camera) {
        (None, None) if shown.is_some() => {
            // ACCameraManager::update: F5 (5) or the cinematic F7 (6) on the scene camera
            let input = gui.orbit_input(session);
            let last = gui.last_frame.unwrap_or(rustyac_game::render::scene::CameraFrame { matrix: rustyac_game::view::IDENTITY, fov: 60.0, near: 0.1 });
            let shot = gui.shot.get_or_insert_with(|| rustyac_game::render::onboard_free::ShotCamera::new(&last));
            let body = rustyac_game::render::scene::model_matrix(&view.body, &session.shape);
            match (shown, gui.cinematic.as_mut(), gui.onboard_free.as_mut()) {
                (Some(4), _, _) => rustyac_game::render::onboard_free::HelicopterCamera::default().update(shot, &body),
                (Some(6), Some(cinematic), _) => {
                    // (Backspace's system message is the GUI's; the page shows none)
                    // Insert: loadSettingsFromINI again (the page's video_editing.ini)
                    if input.held(0x2d) {
                        let [video, track, _] = rustyac_game::render::onboard_free::settings(None);
                        cinematic.read_settings(video.as_ref(), track.as_ref());
                    }
                    if cinematic.blur_switch(&input) {
                        renderer.switch_motion_blur();
                    }
                    cinematic.update(shot, &input, dt);
                }
                // (the page has no AC replay: none loaded, none playing)
                (_, _, Some(f5)) => {
                    f5.update(shot, &body, false, false, [0.0; 2], &input, dt);
                }
                _ => {}
            }
            shot.frame()
        }
        (Some(start), _) => start.update(view, session.shape.eye_model[0], dt),
        (None, Some(free)) => {
            // (the held keys as rustyac.exe's GetAsyncKeyState: either Shift, either Ctrl)
            let keys = &session.input.lock().unwrap().keys;
            let held = |codes: &[i32]| codes.iter().any(|code| keys.contains(code));
            let (x, y, look) = gui.mouse;
            let input = rustyac_game::render::scene::MouseCameraInput { shift: held(&[0x10, 0xa0, 0xa1]), ctrl: held(&[0x11, 0xa2, 0xa3]), up: held(&[0x26]), down: held(&[0x28]), left: held(&[0x25]), right: held(&[0x27]), look, cursor: (x, y) };
            free.update(&input, dt)
        }
        // CameraTrack::update without sets does nothing: the scene camera stays where it was
        (None, None) if session.camera.mode == rustyac_game::render::scene::CameraMode::Track && gui.track_cameras.sets.is_empty() && gui.last_frame.is_some() => gui.last_frame.unwrap(),
        (None, None) if session.camera.mode == rustyac_game::render::scene::CameraMode::Track && !gui.track_cameras.sets.is_empty() => {
            let t = &mut gui.track_cameras;
            session.camera.track_set = t.current_set;
            t.still = gui.paused;
            let body = rustyac_game::render::scene::model_matrix(&view.body, &session.shape);
            let previous = gui.last_frame.as_ref().map_or(rustyac_game::view::IDENTITY, |f| f.matrix);
            let look = t.update(&previous, &body, renderer.steer_place(0), view.lap.position, view.speed_ms, dt).expect("the track has cameras");
            let frame = rustyac_game::render::scene::CameraFrame { matrix: look.matrix, fov: look.fov, near: look.near };
            track_look = Some(look);
            frame
        }
        (None, None) => session.camera_frame(view, seconds as f64),
    };
    gui.track_camera = track_look.as_ref().map_or(String::new(), |look| look.name.clone());
    renderer.set_track_look(track_look.as_ref());
    gui.last_frame = Some(camera);
    let world: Vec<CarView> = std::iter::once(*view).chain(session.remote_views()).collect();
    let mut hud = hud.clone();
    hud.focus_fade = gui.focus.as_ref().and_then(|focus| focus.for_picture());
    renderer.draw(&world, 0, &session.camera, &camera, &hud, seconds);
    // (the page's focus is always the player's car: F4's fade comes back to it)
    if let Some(focus) = &mut gui.focus {
        focus.post_gui(dt, &world, &[]);
        // F3 at its black frame: setMode(eTrack), from the track's cameras CameraTrack::nextSet
        // (F2: and randomMode on)
        if focus.camera_change == Some(rustyac_game::focus::FocusCommand::OnboardFreeCamera) {
            focus.camera_change = None;
            rustyac_game::render::onboard_free::set_mode(5, &mut session.camera, &mut gui.start_camera, &mut gui.free_camera, &mut gui.shot, gui.last_frame.as_ref(), session.shape.car_cameras.len(), gui.cinematic.is_some(), &mut gui.track_cameras);
        }
        if let Some(change) = focus.camera_change.take() {
            let overlay = gui.start_camera.take().is_some() | gui.free_camera.take().is_some() | session.camera.shown.take().is_some();
            if session.camera.f3() && !overlay {
                gui.track_cameras.next_set();
            }
            session.camera.random |= change == rustyac_game::focus::FocusCommand::RandomCamera;
        }
    }
}

/// F7: `system/cfg/assetto_corsa.ini [CAMERA] ALLOW_FREE_CAMERA` above 0 (`Sim.allowFreeCamera`,
/// as rustyac.exe reads it).
pub fn free_camera_allowed(root: &Path) -> bool {
    IniReader::load(&root.join("system/cfg/assetto_corsa.ini")).is_ok_and(|ini| ini.ready && ini.has_section("AC_APPS") && ini.get_int("CAMERA", "ALLOW_FREE_CAMERA").unwrap_or(0) > 0)
}

/// The fixed frame of this drive (as it starts: no physics step yet) at this size: the
/// renderer that drew it and the kgl call log. `gui`: AC's GUI on, and this `--gui-do` script
/// run on it before the first frame.
#[allow(clippy::too_many_arguments)]
pub fn fixed_frame(root: &Path, session: &mut Session, width: u32, height: u32, frames: u32, warp: bool, video: Option<&Path>, gui: Option<(GuiSetup, &str)>, notes: &mut Vec<String>) -> Result<(Box<AcRenderer>, String), String> {
    let view = session.view();
    let setup = &session.sim.setup;
    rustyac_render::kgl_log::begin(&format!("full frame: {} on {} {} from {}, {width} x {height}, {frames} frames of 1/60 s", setup.car, setup.track, setup.layout, setup.spawn));
    let (gui, script) = match gui {
        Some((setup, script)) => (Some(setup), Some(script)),
        None => (None, None),
    };
    // `results` in the script: the referee's end-of-race panel up (RaceManager's EndRace job
    // would put it there at a race's end), for the proofs' pictures
    let script = script.map(|script| {
        let commands: Vec<&str> = script.split(';').map(str::trim).collect();
        if commands.contains(&"results") {
            match session.sim.referee.as_mut() {
                Some(referee) => referee.manager.results_displayed = true,
                None => notes.push("results: no referee (no session list)".to_string()),
            }
        }
        commands.into_iter().filter(|c| *c != "results").collect::<Vec<_>>().join(";")
    });
    let made = renderer(root, width, height, session, warp, true, video, gui, notes).and_then(|mut renderer| {
        if let (Some(gui), Some(script)) = (renderer.gui.as_mut(), script) {
            gui.script(&script)?;
        }
        Ok(renderer)
    });
    let mut renderer = match made {
        Ok(renderer) => renderer,
        Err(why) => {
            rustyac_render::kgl_log::end();
            return Err(why);
        }
    };
    let hud = if renderer.gui.is_some() { hud_info(session, false) } else { HudInfo { hidden: true, ..Default::default() } };
    let mut no_gui_loop = GuiLoop::default();
    for _ in 0..frames {
        draw(&mut renderer, session, &view, FIXED_SECONDS, &hud, &mut no_gui_loop);
    }
    // the GUI's state at the end in the log too: every control (name, shown, place, text, mouse
    // and highlight), the mouse, the desktop, and what the GUI asked of the session
    if let Some(gui) = renderer.gui.as_mut() {
        let actions = gui.take_actions();
        let g = &gui.gui;
        for (i, c) in g.controls.iter().enumerate() {
            let r = [c.rect.left, c.rect.right, c.rect.top, c.rect.bottom].map(f32::to_bits);
            rustyac_render::kgl_log::line(format_args!("gui control {i} {:?} {} {r:?} {:?} {} {} {}", c.name, c.visible, c.text, c.is_mouse_down, c.highlight, c.is_highlight));
        }
        let captured = g.captured_form.map_or("-", |f| g.controls[f].name.as_str());
        rustyac_render::kgl_log::line(format_args!("gui captured {captured} mouse {:?} desktop {} actions {actions:?}", g.mouse_pos.map(f32::to_bits), g.virtual_desktop));
    }
    Ok((renderer, rustyac_render::kgl_log::end()))
}
