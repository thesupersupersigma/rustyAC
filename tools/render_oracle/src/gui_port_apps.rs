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
                let target = root.join("port_files").join(path.replace('\\', "/"));
                std::fs::create_dir_all(target.parent().unwrap_or(root)).map_err(|e| e.to_string())?;
                std::fs::write(&target, bytes).map_err(|e| format!("{}: {e}", target.display()))?;
            }
            AppAction::WriteProfileString { file, section, key, value } => {
                let target = docs.join(&file);
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
