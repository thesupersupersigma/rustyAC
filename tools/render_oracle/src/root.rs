// SPDX-License-Identifier: GPL-3.0-or-later

//! The oracle's scratch game folder, and the Task 20 profile. The game's code builds relative
//! paths (`system/cfg/…`, `system/shaders/win/…`, `content/weather/…`, `cfg/video.ini`), so
//! the oracle runs with its current directory in a folder of its own that holds copies of
//! those small files, and its own `cfg/video.ini`. The game's folder itself is only ever read;
//! the big files (kn5 models) are opened there through absolute paths.

use std::path::Path;

use crate::Args;

/// The values of `cfg/video.ini` both sides run with. The default is the Task 20 profile.
#[derive(Clone, Copy, Debug)]
pub struct Profile {
    pub anisotropic: i32,
    pub shadow_map_size: i32,
    pub world_detail: i32,
    pub cubemap_size: i32,
    pub cubemap_faces_per_frame: i32,
}

pub const TASK_20: Profile = Profile { anisotropic: 8, shadow_map_size: 2048, world_detail: 5, cubemap_size: 512, cubemap_faces_per_frame: 0 };

static PROFILE: std::sync::OnceLock<Profile> = std::sync::OnceLock::new();

pub fn set_profile(profile: Profile) {
    let _ = PROFILE.set(profile);
}

pub fn profile() -> Profile {
    *PROFILE.get().unwrap_or(&TASK_20)
}

/// `cfg/video.ini` of the proof: no post-processing, no motion blur, no MSAA, no mirror, no
/// smoke, a cube map that renders no faces per frame, shadows on at a fixed size.
pub fn video_ini(width: u32, height: u32) -> String {
    let Profile { anisotropic: ANISOTROPIC, shadow_map_size: SHADOW_MAP_SIZE, world_detail: WORLD_DETAIL, cubemap_size: CUBEMAP_SIZE, cubemap_faces_per_frame: CUBEMAP_FACES_PER_FRAME } = profile();
    let lines = [
        "[VIDEO]".to_string(),
        format!("WIDTH={width}"),
        format!("HEIGHT={height}"),
        "REFRESH=60".into(),
        "FULLSCREEN=0".into(),
        "VSYNC=0".into(),
        "AASAMPLES=1".into(),
        "AAQUALITY=0".into(),
        format!("ANISOTROPIC={ANISOTROPIC}"),
        format!("SHADOW_MAP_SIZE={SHADOW_MAP_SIZE}"),
        "FPS_CAP_MS=0".into(),
        "INDEX=0".into(),
        String::new(),
        "[REFRESH]".into(),
        "VALUE=60".into(),
        String::new(),
        "[CAMERA]".into(),
        "MODE=DEFAULT".into(),
        String::new(),
        "[ASSETTOCORSA]".into(),
        "HIDE_ARMS=0".into(),
        "HIDE_STEER=0".into(),
        "LOCK_STEER=0".into(),
        format!("WORLD_DETAIL={WORLD_DETAIL}"),
        String::new(),
        "[EFFECTS]".into(),
        "MOTION_BLUR=0".into(),
        "RENDER_SMOKE_IN_MIRROR=0".into(),
        "SMOKE=0".into(),
        "FXAA=0".into(),
        String::new(),
        "[POST_PROCESS]".into(),
        "ENABLED=0".into(),
        "QUALITY=0".into(),
        "FILTER=default".into(),
        "GLARE=0".into(),
        "DOF=0".into(),
        "RAYS_OF_GOD=0".into(),
        "HEAT_SHIMMER=0".into(),
        "FXAA=0".into(),
        String::new(),
        "[MIRROR]".into(),
        "HQ=0".into(),
        "SIZE=0".into(),
        String::new(),
        "[CUBEMAP]".into(),
        format!("SIZE={CUBEMAP_SIZE}"),
        format!("FACES_PER_FRAME={CUBEMAP_FACES_PER_FRAME}"),
        "FARPLANE=0".into(),
        String::new(),
        "[SATURATION]".into(),
        "LEVEL=100".into(),
        String::new(),
    ];
    lines.join("\n")
}

fn copy_dir(from: &Path, to: &Path, deep: bool) -> Result<(), String> {
    let io = |e: std::io::Error| format!("{} -> {}: {e}", from.display(), to.display());
    std::fs::create_dir_all(to).map_err(io)?;
    for entry in std::fs::read_dir(from).map_err(io)? {
        let entry = entry.map_err(io)?;
        let target = to.join(entry.file_name());
        let kind = entry.file_type().map_err(io)?;
        if kind.is_dir() {
            if deep {
                copy_dir(&entry.path(), &target, true)?;
            }
        } else {
            // only when it differs: a second oracle process may be reading the file right now
            let same = match (std::fs::metadata(&target), entry.metadata()) {
                (Ok(a), Ok(b)) => a.len() == b.len(),
                _ => false,
            };
            if !same {
                std::fs::copy(entry.path(), &target).map_err(io)?;
            }
        }
    }
    Ok(())
}

pub fn prepare(args: &Args) -> Result<(), String> {
    let root = &args.root;
    if root.join("acs.exe").is_file() || root.join("AssettoCorsa.exe").is_file() {
        return Err(format!("{} looks like a game folder: the oracle root has to be a scratch folder of its own", root.display()));
    }
    let game = &args.game;
    copy_dir(&game.join("system/cfg"), &root.join("system/cfg"), false)?;
    copy_dir(&game.join("system/shaders/win"), &root.join("system/shaders/win"), false)?;
    copy_dir(&game.join("content/weather"), &root.join("content/weather"), true)?;
    std::fs::create_dir_all(root.join("cfg")).map_err(|e| e.to_string())?;
    let video = video_ini(args.width, args.height);
    let same = std::fs::read_to_string(root.join("cfg/video.ini")).is_ok_and(|old| old == video);
    if !same {
        std::fs::write(root.join("cfg/video.ini"), video).map_err(|e| e.to_string())?;
    }
    // what the game's SkyBox constructor reads (the clouds of the weather)
    let race = format!("[WEATHER]\nNAME={}\n\n[LIGHTING]\nSUN_ANGLE={}\nTIME_MULT=1\nCLOUD_SPEED=0.2\n", args.weather, args.sun_angle);
    let same = std::fs::read_to_string(root.join("cfg/race.ini")).is_ok_and(|old| old == race);
    if !same {
        std::fs::write(root.join("cfg/race.ini"), race).map_err(|e| e.to_string())?;
    }
    Ok(())
}
