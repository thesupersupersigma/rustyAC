// SPDX-License-Identifier: GPL-3.0-or-later

//! The port side of the apps' proofs: `rustyac_gui`'s app forms, fed the same tapes.

use rustyac_gui::apps::{AppAction, MixLevel};
use rustyac_gui::control::Id;
use rustyac_gui::gui::Gui;
use rustyac_gui::painter::Painter;

#[derive(Default)]
pub struct AppsPort {
    pub made: Vec<(String, Id)>,
}

fn app(apps: &AppsPort, name: &str) -> Result<Id, String> {
    apps.made.iter().find(|a| a.0 == name).map(|a| a.1).ok_or(format!("no app {name:?} was made"))
}

/// The game's folder (`content/...` and `system/cfg/...` are read from there by the apps).
pub fn make(gui: &mut Gui, painter: &mut dyn Painter, game_folder: &std::path::Path, screen: Id, name: &str) -> Result<Id, String> {
    let read = |path: std::path::PathBuf| rustyac_physics::data::ini::IniReader::load(&path).ok().filter(|ini| ini.ready);
    Ok(match name {
        "tyres" => gui.new_tyres_app(painter, read(game_folder.join("system/cfg/tyres_app.ini")).as_ref()),
        "chat" => gui.new_chat_app(painter, read(game_folder.join("system/cfg/chat_app.ini")).as_ref(), crate::gui_seq_apps::chat_clock),
        "pedals" => gui.new_pedals_app(painter, read(game_folder.join("documents/Assetto Corsa/cfg/controls.ini")).as_ref()),
        "electronics" => gui.new_electronics_app(painter),
        "help" => {
            let text = std::fs::read(game_folder.join(format!("system/locales/help/{}.tag", crate::gui_seq::language_or_en()))).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
            gui.new_help_app(painter, &text)
        }
        "hideall" => gui.new_hide_all_app(painter, screen),
        "runningtime" => {
            let ini = read(game_folder.join("documents/Assetto Corsa/cfg/time_limited_test.ini"));
            gui.new_running_time_app(painter, ini.map_or(0, |i| i.get_int("SETTINGS", "MAX_RUNNING_TIME").unwrap_or(0)))
        }
        "timeflow" => gui.new_time_flow_app(painter),
        "trackdescription" => gui.new_track_description_app(painter, crate::gui_seq_apps::script(|s| s.sections.clone())),
        "trackmap" => {
            let (track, config) = crate::gui_seq_apps::script(|s| s.track.clone());
            let map_ini = read(game_folder.join("system/cfg/map.ini"));
            let folder = if config.is_empty() { game_folder.join("content/tracks").join(&track) } else { game_folder.join("content/tracks").join(&track).join(&config) };
            let track_map_ini = read(folder.join("data/map.ini"));
            let points = crate::gui_seq_apps::script(|s| s.ai_points.clone());
            let files = rustyac_gui::app_track_map::TrackMapFiles { map_ini: map_ini.as_ref(), track_map_ini: track_map_ini.as_ref(), track: &track, config: &config, ai_points: &points };
            gui.new_track_map_app(painter, &files)
        }
        "gears" => {
            let engines: Vec<_> = gui.apps.cars.iter().map(|c| read(game_folder.join("content/cars").join(&c.unix_name).join("data/engine.ini"))).collect();
            gui.new_gears_app(painter, &engines)
        }
        "laps" => gui.new_laps_app(painter),
        "rstats" => gui.new_render_stats_app(painter),
        "driverperf" => {
            let ini = read(game_folder.join("system/cfg/driver_performances.ini"));
            gui.new_driver_performance_app(painter, ini.as_ref(), None)
        }
        "fuelmonitor" => gui.new_race_fuel_monitor_app(painter),
        "sustelemetry" => gui.new_sus_telemetry_app(painter),
        "tyredebug" => gui.new_tyre_debug_app(painter),
        "setupdebug" => gui.new_setup_debug_app(painter),
        "carphysics" => gui.new_car_physics_app(painter),
        "carengineer" => gui.new_car_engineer_app(painter),
        "wings" => gui.new_wings_app(painter),
        "drszones" => gui.new_drs_zones_app(painter),
        "chasecameditor" => gui.new_chase_cam_editor_app(painter),
        "netmonitor" => gui.new_net_monitor_app(painter),
        "keyboard" => gui.new_keyboard_debug_app(painter),
        "x360" => gui.new_pad_debug_app(painter),
        "photomode" => {
            let start = crate::gui_seq_apps::photo_start();
            gui.apps.photo = start;
            *PHOTO.lock().unwrap() = Some((start, Vec::new(), String::new()));
            gui.new_photo_mode_app(painter)
        }
        "cameraeditor" => {
            let start = crate::gui_seq_apps::camera_editor_start();
            gui.apps.camera_editor = Some(start.clone());
            *CAMERA.lock().unwrap() = Some((start, Vec::new(), String::new()));
            gui.new_camera_editor_app(painter, 0.0)
        }
        "gpuprofiler" => gui.new_gpu_profiler_app(painter),
        "suspension" => gui.new_suspension_app(painter),
        "tyretester" => {
            let unix = crate::gui_seq_apps::script(|s| s.unix.clone());
            gui.new_tyre_tester_app(painter, &game_folder.join("content/cars").join(unix).join("data"))?
        }
        "telemetry" => {
            let presets = read(game_folder.join("system/cfg/telemetry_presets.ini"));
            let user = read(game_folder.join("documents/Assetto Corsa/cfg/telemetry_user.ini"));
            gui.new_telemetry_app(painter, &crate::gui_seq_apps::TELEMETRY_CAR, presets.as_ref(), user.as_ref())
        }
        "gamestats" => gui.new_game_stats_app(painter),
        "position" => gui.new_race_position_app(painter),
        "essentials" => gui.new_essentials_app(painter),
        "delta" => gui.new_performance_delta_app(painter),
        "realtime" => gui.new_realtime_app(painter),
        "volumes" => gui.new_volumes_app(painter),
        "mgu" => gui.new_mgu_app(painter),
        "friends" => gui.new_friends_app(painter),
        "eyes" => {
            let (e, fov, pitch) = crate::gui_seq_apps::script(|s| s.eyes).ok_or("app.eyes first")?;
            *EYES.lock().unwrap() = Some(EyesPort { eyes: e, fov, pitch, log: Vec::new(), last: String::new() });
            sync(gui);
            let unix = gui.apps.cars.first().map(|c| c.unix_name.clone()).unwrap_or_default();
            let car_ini = game_folder.join(format!("content/cars/{unix}/data/car.ini"));
            let car = read(car_ini.clone());
            let onboard = read(game_folder.join("cfg/camera_onboard.ini"));
            let defaults = rustyac_gui::app_eyes::EyesDefaults {
                eyes: car.as_ref().map(|i| i.get_float3("GRAPHICS", "DRIVEREYES").unwrap_or([0.0; 3])),
                pitch_degrees: car.as_ref().map(|i| i.get_float("GRAPHICS", "ON_BOARD_PITCH_ANGLE").unwrap_or(0.0)),
                fov: onboard.as_ref().map(|i| i.get_float("MODE", "FOV").unwrap_or(0.0)),
            };
            gui.new_eyes_app(painter, defaults, car_ini.is_file())
        }
        "ballast" => {
            *BALLAST.lock().unwrap() = Some((0.0, String::new()));
            gui.new_ballast_app(painter)
        }
        "ffb" => {
            let unix = gui.apps.cars.first().map(|c| c.unix_name.clone()).unwrap_or_default();
            let loose = game_folder.join(format!("content/cars/{unix}/data/car.ini")).is_file();
            let upgrades = crate::gui_seq_apps::script(|s| s.ff_upgrades);
            ffb(|f| *f = Some(FfbPort::new()));
            sync(gui);
            gui.new_ffb_app(painter, loose, upgrades)
        }
        _ => return Err(format!("no app {name:?} in the port")),
    })
}

pub fn call(apps: &mut AppsPort, gui: &mut Gui, painter: &mut dyn Painter, game_folder: &std::path::Path, screen: Id, what: &str, number: f32, text: &str) -> Result<(), String> {
    sync(gui);
    match what {
        "app.create" => {
            if crate::gui_seq_apps::script(|s| s.sectors) {
                gui.micro_sectors = rustyac_gui::micro_sectors::MicroSectors::new(Some(1));
            }
        }
        "app.new" => {
            // (the game's side hears the widgets only once they are made)
            let heard = gui.widget_events.len();
            let form = make(gui, painter, game_folder, screen, text)?;
            gui.widget_events.truncate(heard);
            gui.game_screen_add_control(painter, screen, form, number == 0.0);
            write_files(gui, game_folder)?;
            apps.made.push((text.to_string(), form));
        }
        "app.shutdown" => {
            gui.shutdown_apps();
            write_files(gui, game_folder)?;
        }
        "app.visible" => {
            let form = app(apps, text)?;
            gui.set_visible(form, number != 0.0);
        }
        other => return Err(format!("the port's side knows no {other:?}")),
    }
    Ok(())
}

/// What the apps asked of the session: the files into `port_files` of the scratch root (the
/// settings files into its `documents`), the sound's settings into [`SOUND`].
fn write_files(gui: &mut Gui, root: &std::path::Path) -> Result<(), String> {
    let docs = root.join("port_files/documents/Assetto Corsa");
    for action in std::mem::take(&mut gui.apps.actions) {
        match action {
            AppAction::WriteFile { path, bytes } => {
                // (a settings file under Documents, a track's map under the game's folder)
                let path = path.replace('\\', "/");
                let target = if path.starts_with("cfg/") { docs.join(&path) } else { root.join("port_files").join(&path) };
                std::fs::create_dir_all(target.parent().unwrap_or(root)).map_err(|e| e.to_string())?;
                std::fs::write(&target, bytes).map_err(|e| format!("{}: {e}", target.display()))?;
            }
            AppAction::WriteProfileString { file, section, key, value } => {
                // (a car's car.ini, system/cfg/chase_cam.ini: a file of the game's folder)
                let target = if file.starts_with("content/") || file.starts_with("system/") { root.join("port_files").join(&file) } else { docs.join(&file) };
                std::fs::create_dir_all(target.parent().unwrap_or(root)).map_err(|e| e.to_string())?;
                let old = std::fs::read(&target).ok();
                let bytes = rustyac_gui::apps::write_profile_string(old.as_deref(), &section, &key, &value);
                std::fs::write(&target, bytes).map_err(|e| format!("{}: {e}", target.display()))?;
            }
            AppAction::SetMasterVolume(v) => sound(|s| s.log.push(format!("master {:08x}", v.to_bits()))),
            AppAction::SetMixLevel { level, value } => sound(|s| {
                for mix in s.mix.iter_mut() {
                    let k = level as usize;
                    // MixLevel's order is MixVolumes' (wind, tyres, surfaces, dirt, engine,
                    // transmission, opponents); the transmission's -1 copies the engine's
                    mix[k] = if level == MixLevel::Transmission { if value == -1.0 { mix[4] } else { value * 3.0 } } else { value };
                }
            }),
            AppAction::SetCameraSound { car, which, external } => sound(|s| {
                if let Some(c) = s.camera.get_mut(car) {
                    c[which] = external;
                }
            }),
            AppAction::SetUserFfGain(v) => ffb(|f| {
                if let Some(f) = f {
                    f.avatar = v;
                    f.car = v;
                }
            }),
            AppAction::SetFfMult(v) => ffb(|f| {
                if let Some(f) = f {
                    f.mult = v;
                }
            }),
            AppAction::SetFfUpgrade { which, value } => ffb(|f| {
                if let Some(u) = f.as_mut().and_then(|f| f.upgrades.as_mut()) {
                    u[which.min(3)] = value;
                }
            }),
            // (the port's app keeps its own copy in gui.apps.keyboard; the pictures show it)
            AppAction::SetKeyboardTuning { .. } | AppAction::SetPadTuning { .. } => {}
            AppAction::SetCameraMode(mode) => photo_log(format!("setMode {mode}")),
            AppAction::PausePhysics(paused) => photo_log(format!("pausePhysics {}", paused as u8)),
            AppAction::TakeScreenshot => photo_log("takeScreen".to_string()),
            AppAction::SelectCamera { mode, index } => {
                if let Some(c) = CAMERA.lock().unwrap().as_mut() {
                    c.1.push(format!("setMode {mode}"));
                    c.1.push(if mode == 0 { format!("setDrivableCarIndex {index}") } else { format!("setCameraCarIndex {index}") });
                }
            }
            AppAction::DeleteFile { path } => {
                let _ = std::fs::remove_file(docs.join(path));
            }
            AppAction::SetDriverEyes(e) => {
                if let Some(s) = EYES.lock().unwrap().as_mut() {
                    s.eyes = e;
                }
            }
            AppAction::SetOnboardFov(f) => {
                if let Some(s) = EYES.lock().unwrap().as_mut() {
                    s.fov = f;
                }
            }
            AppAction::SetOnboardPitch(p) => {
                if let Some(s) = EYES.lock().unwrap().as_mut() {
                    s.pitch = p;
                }
            }
            AppAction::SetBallastKg(kg) => {
                if let Some(b) = BALLAST.lock().unwrap().as_mut() {
                    b.0 = kg;
                }
            }
            AppAction::AddMessage { title, text } => ffb(|f| {
                if let Some(f) = f {
                    f.log.push(format!("message {title}|{text}"));
                } else if let Some(s) = EYES.lock().unwrap().as_mut() {
                    s.log.push(format!("message {title}|{text}"));
                } else {
                    append_effects(&root.join("effects_port.txt"), vec![format!("message {title}|{text}")], "", &mut String::new());
                }
            }),
        }
    }
    Ok(())
}

/// The sound's settings as the port's apps left them: the master level's calls, every car's
/// `MixVolumes`, every car's views' sounds.
#[derive(Default)]
pub struct Sound {
    pub log: Vec<String>,
    pub mix: Vec<[f32; 7]>,
    pub camera: Vec<[bool; 2]>,
    pub made: bool,
    pub last: String,
}

static SOUND: std::sync::Mutex<Option<Sound>> = std::sync::Mutex::new(None);

fn sound<T>(f: impl FnOnce(&mut Sound) -> T) -> T {
    f(SOUND.lock().unwrap().get_or_insert_with(Sound::default))
}

/// The sound's settings as a line of `effects_*.txt` (the same text on the two sides).
pub fn sound_line(mix: &[[f32; 7]], camera: &[[bool; 2]]) -> String {
    let mut line = String::new();
    for (i, m) in mix.iter().enumerate() {
        line += &format!("car {i} mix {}", m.iter().map(|v| format!("{:08x}", v.to_bits())).collect::<Vec<_>>().join(" "));
        if let Some(c) = camera.get(i) {
            line += &format!(" bonnet {} bumper {}", c[0] as u8, c[1] as u8);
        }
        line += "; ";
    }
    line
}

/// The force feedback app's session on the port's side: the avatar's and the car's gains, the
/// car's FF mult and steer assist, the wheel's effects, the messages.
pub struct FfbPort {
    pub avatar: f32,
    pub car: f32,
    pub mult: f32,
    pub assist: f32,
    pub upgrades: Option<[f32; 4]>,
    pub log: Vec<String>,
    pub last: String,
}

impl FfbPort {
    fn new() -> FfbPort {
        let (mult, assist, upgrades) = crate::gui_seq_apps::script(|s| (s.ff_mult, s.steer_assist, s.ff_upgrades));
        FfbPort { avatar: 1.0, car: 1.0, mult, assist, upgrades, log: Vec::new(), last: String::new() }
    }
}

static FFB: std::sync::Mutex<Option<FfbPort>> = std::sync::Mutex::new(None);

/// Onboard settings' session on the port's side.
pub struct EyesPort {
    pub eyes: [f32; 3],
    pub fov: f32,
    pub pitch: f32,
    pub log: Vec<String>,
    pub last: String,
}

static EYES: std::sync::Mutex<Option<EyesPort>> = std::sync::Mutex::new(None);

/// The on-board camera's settings as a line of `effects_*.txt`.
pub fn eyes_line(e: [f32; 3], fov: f32, pitch: f32) -> String {
    format!("eyes {:08x} {:08x} {:08x} fov {:08x} pitch {:08x}", e[0].to_bits(), e[1].to_bits(), e[2].to_bits(), fov.to_bits(), pitch.to_bits())
}

/// The Camera editor's: what its app changed, the camera manager's calls, the last line.
static CAMERA: std::sync::Mutex<Option<(rustyac_gui::app_camera_editor::CameraEditorLink, Vec<String>, String)>> = std::sync::Mutex::new(None);

/// The Camera editor's effects as a line of `effects_*.txt` (every float's bits).
pub fn camera_line(l: &rustyac_gui::app_camera_editor::CameraEditorLink) -> String {
    let bits = |v: &[f32]| v.iter().map(|x| format!("{:08x}", x.to_bits())).collect::<Vec<_>>().join(" ");
    let mut line = format!(
        "camera exp {} eyes {} pitch {} bumper {} {} bonnet {} {} scene {} limits {}",
        bits(&[l.on_board_exposure, l.out_board_exposure, l.dash_board_exposure]),
        bits(&l.eyes),
        bits(&[l.onboard_pitch]),
        bits(&l.bumper_pos),
        bits(&[l.bumper_pitch]),
        bits(&l.bonnet_pos),
        bits(&[l.bonnet_pitch]),
        bits(l.scene_camera.m.as_flattened()),
        l.use_auto_exposure_limits as u8
    );
    for (i, c) in l.cameras.iter().enumerate() {
        line += &format!(" | cam{i} {} fov {} exp {} sound {}", bits(c.matrix.m.as_flattened()), bits(&[c.fov]), bits(&[c.exposure]), c.external_sound as u8);
    }
    line
}

/// The Photo mode's: what its app changed, the calls, the last line.
static PHOTO: std::sync::Mutex<Option<(rustyac_gui::app_photo_mode::PhotoLink, Vec<String>, String)>> = std::sync::Mutex::new(None);

fn photo_log(line: String) {
    if let Some(p) = PHOTO.lock().unwrap().as_mut() {
        p.1.push(line);
    }
}

/// The Photo mode's effects as a line of `effects_*.txt` (every float's bits); `animator`: the
/// sun animator still runs.
pub fn photo_line(l: &rustyac_gui::app_photo_mode::PhotoLink, animator: bool) -> String {
    let bits = |v: &[f32]| v.iter().map(|x| format!("{:08x}", x.to_bits())).collect::<Vec<_>>().join(" ");
    let sun = l.custom_sun.map_or("off".to_string(), |s| bits(&[s.x, s.y, s.z]));
    format!(
        "photo cam {} fov {} dof {} exp {} mode {} sun {} animator {}",
        bits(l.matrix.m.as_flattened()),
        bits(&[l.fov]),
        bits(&[l.dof_factor, l.dof_focus]),
        bits(&[l.exposure_multiplier]),
        l.camera_mode,
        sun,
        animator as u8
    )
}

/// The ballast app's: the player's car's ballast as the port's app set it, the last line.
static BALLAST: std::sync::Mutex<Option<(f32, String)>> = std::sync::Mutex::new(None);

fn ffb<T>(f: impl FnOnce(&mut Option<FfbPort>) -> T) -> T {
    f(&mut FFB.lock().unwrap())
}

/// The force feedback's settings as a line of `effects_*.txt`.
pub fn ffb_line(avatar: f32, car: f32, mult: f32, upgrades: Option<[f32; 4]>) -> String {
    let mut line = format!("ffb avatar {:08x} car {:08x} mult {:08x}", avatar.to_bits(), car.to_bits(), mult.to_bits());
    if let Some(u) = upgrades {
        line += &format!(" upgrades {}", u.iter().map(|v| format!("{:08x}", v.to_bits())).collect::<Vec<_>>().join(" "));
    }
    line
}

/// What the apps read, as the script has it now.
pub fn sync(gui: &mut Gui) {
    let documents = crate::root::documents();
    let root = documents.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let _ = write_files(gui, &root);
    let was_replay = gui.apps.in_replay;
    // (the ChaseCam Editor's spinners change the camera: kept in the script)
    let chase = gui.apps.chase_cam;
    if chase != (0.0, 0.0, 0.0) {
        crate::gui_seq_apps::script(|s| s.chase = chase);
    }
    // (Keyboard debug's spinners change the keyboard's first five parameters: kept in the script)
    if let Some(k) = gui.apps.keyboard {
        crate::gui_seq_apps::script(|s| {
            if let Some(s) = s.keyboard.as_mut() {
                s[..5].copy_from_slice(&k[..5]);
            }
        });
    }
    if let Some(p) = gui.apps.pad {
        crate::gui_seq_apps::script(|s| {
            if let Some(s) = s.pad.as_mut() {
                s[..5].copy_from_slice(&p[..5]);
            }
        });
    }
    let camera = gui.apps.camera_editor.take();
    let photo = gui.apps.photo;
    gui.apps = crate::gui_seq_apps::link();
    // the Photo mode's: what the port's app changed, the buttons from the script
    if let Some(p) = PHOTO.lock().unwrap().as_mut() {
        p.0 = photo;
        [p.0.right_button, p.0.middle_button] = crate::gui_seq_apps::script(|s| s.photo_keys);
        gui.apps.photo = p.0;
        let line = photo_line(&p.0, p.0.custom_sun.is_none());
        append_effects(&root.join("effects_port.txt"), std::mem::take(&mut p.1), &line, &mut p.2);
    }
    // the Camera editor's: what the port's app changed, kept here
    if let Some(c) = CAMERA.lock().unwrap().as_mut() {
        if let Some(l) = camera {
            c.0 = l;
        }
        c.0.auto_exposure = crate::gui_seq_apps::script(|s| s.auto_exposure);
        // the oracle moves car 0's body every frame (the game's node too)
        if let Some(car) = gui.apps.cars.first() {
            c.0.body = car.body_matrix;
        }
        gui.apps.camera_editor = Some(c.0.clone());
        let line = camera_line(&c.0);
        append_effects(&root.join("effects_port.txt"), std::mem::take(&mut c.1), &line, &mut c.2);
    }
    if gui.apps.in_replay != was_replay {
        gui.apps_on_replay_status(gui.apps.in_replay);
    }
    // the force feedback app's: the player's car as the port's apps left it
    let state = ffb(|f| f.as_ref().map(|f| (f.avatar, f.mult, f.assist, f.upgrades)));
    if let Some((avatar, mult, assist, upgrades)) = state {
        if let Some(car) = gui.apps.cars.first_mut() {
            car.has_physics = true;
            car.avatar_ff_gain = avatar;
            car.ff_mult = mult;
            car.steer_assist = assist;
        }
        gui.apps.ff_upgrades = upgrades;
        let path = root.join("effects_port.txt");
        ffb(|f| {
            if let Some(f) = f {
                let line = ffb_line(f.avatar, f.car, f.mult, f.upgrades);
                append_effects(&path, std::mem::take(&mut f.log), &line, &mut f.last);
            }
        });
    }
    if let Some(s) = EYES.lock().unwrap().as_mut() {
        gui.apps.driver_eyes = s.eyes;
        gui.apps.onboard_fov = s.fov;
        gui.apps.onboard_pitch = s.pitch;
        let line = eyes_line(s.eyes, s.fov, s.pitch);
        append_effects(&root.join("effects_port.txt"), std::mem::take(&mut s.log), &line, &mut s.last);
    }
    if let Some((kg, last)) = BALLAST.lock().unwrap().as_mut() {
        let line = format!("ballast {:08x}", kg.to_bits());
        append_effects(&root.join("effects_port.txt"), Vec::new(), &line, last);
    }
    // the Audio app's: the views as the port's apps left them, Documents cfg/audio.ini
    let volumes = gui.controls.iter().any(|c| c.name == "FORM_VOLUMES");
    if volumes {
        let (camera, line, log) = sound(|s| {
            if !s.made {
                s.made = true;
                s.camera = gui.apps.camera_sounds.clone();
                s.camera.resize(gui.apps.cars.len(), [false; 2]);
                s.mix = vec![[0.0; 7]; gui.apps.cars.len()];
            }
            let line = sound_line(&s.mix, &s.camera);
            (s.camera.clone(), line, std::mem::take(&mut s.log))
        });
        gui.apps.camera_sounds = camera;
        gui.apps.audio_ini = std::fs::read(root.join("port_files/documents/Assetto Corsa/cfg/audio.ini")).ok().map(|b| String::from_utf8_lossy(&b).into_owned());
        append_effects(&root.join("effects_port.txt"), log, &line, &mut sound(|s| std::mem::take(&mut s.last)));
        sound(|s| s.last = line);
    }
}

/// The master level's calls, then the settings' line when it changed.
pub fn append_effects(path: &std::path::Path, log: Vec<String>, line: &str, last: &mut String) {
    use std::io::Write;
    let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) else { return };
    for l in log {
        let _ = writeln!(f, "{l}");
    }
    if line != last {
        let _ = writeln!(f, "{line}");
        *last = line.to_string();
    }
}
