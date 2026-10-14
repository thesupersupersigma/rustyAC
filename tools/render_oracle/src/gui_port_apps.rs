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
        "pedals" => gui.new_pedals_app(painter, read(game_folder.join("documents/Assetto Corsa/cfg/controls.ini")).as_ref()),
        "electronics" => gui.new_electronics_app(painter),
        "help" => {
            let text = std::fs::read(game_folder.join("system/locales/help/en.tag")).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
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
        "position" => gui.new_race_position_app(painter),
        "essentials" => gui.new_essentials_app(painter),
        "delta" => gui.new_performance_delta_app(painter),
        "realtime" => gui.new_realtime_app(painter),
        "volumes" => gui.new_volumes_app(painter),
        "mgu" => gui.new_mgu_app(painter),
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
        "app.create" => {}
        "app.new" => {
            let form = make(gui, painter, game_folder, screen, text)?;
            gui.game_screen_add_control(painter, screen, form, number == 0.0);
            write_files(gui, game_folder)?;
            apps.made.push((text.to_string(), form));
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
                // (a car's car.ini: a file of the game's folder)
                let target = if file.starts_with("content/") { root.join("port_files").join(&file) } else { docs.join(&file) };
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
    gui.apps = crate::gui_seq_apps::link();
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
