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
    /// `[CUBEMAP] FARPLANE`; 0 leaves the cube-map camera's 350 m
    pub cubemap_far_plane: f32,
    /// `[EFFECTS] SMOKE`: 0 (off) to 5
    pub smoke: i32,
    /// `[MIRROR] SIZE` (0: no mirrors), `[EFFECTS] RENDER_SMOKE_IN_MIRROR`, and
    /// `gameplay.ini [VIRTUAL_MIRROR] ACTIVE`
    pub mirror_size: i32,
    pub mirror_smoke: bool,
    pub virtual_mirror: bool,
    /// `[MIRROR] HQ`: 0, or the sample count the mirror's multisampled target is made with (the
    /// screen's `AASAMPLES` in the game; the oracle's screen stays at one sample)
    pub mirror_hq: i32,
    /// `[VIDEO] AASAMPLES`: samples per pixel of the screen (Task 23)
    pub aa_samples: i32,
    /// `[POST_PROCESS] ENABLED`: the game's post-processing camera (`CameraForwardYebis`) with
    /// YEBIS itself replaced by a plain copy on both sides, and the engine's HDR levels
    pub post: bool,
    /// `[POST_PROCESS] FXAA` (only with `post`)
    pub fxaa: bool,
    /// `[EFFECTS] MOTION_BLUR`, 0 to 12 (only with `post`)
    pub motion_blur: i32,
    /// with `post`: the light levels of post-processing off (the game's flag is switched off
    /// around everything but the camera's frame; the port's `VideoSettings::ldr_levels`)
    pub ldr_levels: bool,
    /// Task 47: `[CAMERA] MODE=TRIPLE` (the game's `CameraTripleScreen`) with [`TRIPLE_SCREEN_INI`]
    pub triple: bool,
    /// Task 47b: `[CAMERA] MODE=OPENVR` (the game's `StereoCameraVive`) on the stand-in runtime,
    /// with [`OPENVR_INI`]
    pub vr: bool,
    /// Task 47b: with `vr`, `[CAMERA] MODE=OCULUS` (the game's `StereoCameraForward`) on the
    /// stand-in Oculus runtime, with [`OCULUS_INI`]
    pub oculus: bool,
}

/// Documents `cfg/openvr.ini` of a `--vr` proof: the window shows the left eye, and a distance
/// of the eyes other than SteamVR's, so the game moves SteamVR's `ipdOffset`.
pub const OPENVR_INI: &str = "[MIRROR_TEXTURE]\nENABLED=1\n\n[SETTINGS]\nPUPILLARY_DISTANCE=66\n";

/// Documents `cfg/oculus.ini` of an `--oculus` proof: the player's file as the game's "Oculus"
/// settings write it, the window showing the left eye.
pub const OCULUS_INI: &str = "[SETTINGS]\nAUTOSELECT_RIFT_AUDIO_DEVICE=0\nPIXEL_PER_DISPLAY=1.0\n\n[MIRROR_TEXTURE]\nENABLED=1\n";

/// Documents `cfg/triple_screen.ini` of a `--triple` proof: the file the game's "Triple screen"
/// app writes at its start values (no bezel, 1000 mm screens 1000 mm away, side screens turned
/// in a little), as it is on Super's PC.
pub const TRIPLE_SCREEN_INI: &str = "[LEFT_SCREEN]\nLEFT_POINT=-1.49863,0.947665\nRIGHT_POINT=-0.5,1\n\n[CENTER_SCREEN]\nLEFT_POINT=-0.5,1\nRIGHT_POINT=0.5,1\n\n[RIGHT_SCREEN]\nLEFT_POINT=0.5,1\nRIGHT_POINT=1.49863,0.947665\n";

pub const TASK_20: Profile = Profile { anisotropic: 8, shadow_map_size: 2048, world_detail: 5, cubemap_size: 512, cubemap_faces_per_frame: 0, cubemap_far_plane: 0.0, smoke: 0, mirror_size: 0, mirror_smoke: false, virtual_mirror: false, mirror_hq: 0, aa_samples: 1, post: false, fxaa: false, motion_blur: 0, ldr_levels: false, triple: false, vr: false, oculus: false };

static PROFILE: std::sync::OnceLock<Profile> = std::sync::OnceLock::new();
static DOCUMENTS: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// The folder both sides take for the user's Documents: `<scratch root>/documents` (empty).
pub fn set_documents(root: &Path) {
    let _ = DOCUMENTS.set(root.join("documents"));
}

pub fn documents() -> std::path::PathBuf {
    DOCUMENTS.get().cloned().unwrap_or_default()
}

pub fn set_profile(profile: Profile) {
    let _ = PROFILE.set(profile);
}

pub fn profile() -> Profile {
    *PROFILE.get().unwrap_or(&TASK_20)
}

/// `cfg/video.ini` of the proof: no post-processing, no motion blur, no MSAA, no mirror, no
/// smoke, a cube map that renders no faces per frame, shadows on at a fixed size.
#[allow(non_snake_case)]
pub fn video_ini(width: u32, height: u32) -> String {
    let Profile { anisotropic: ANISOTROPIC, shadow_map_size: SHADOW_MAP_SIZE, world_detail: WORLD_DETAIL, cubemap_size: CUBEMAP_SIZE, cubemap_faces_per_frame: CUBEMAP_FACES_PER_FRAME, cubemap_far_plane: CUBEMAP_FARPLANE, smoke: SMOKE, mirror_size: MIRROR_SIZE, mirror_smoke, virtual_mirror: _, mirror_hq, aa_samples: AASAMPLES, post, fxaa, motion_blur: MOTION_BLUR, ldr_levels: _, triple, vr, oculus } = profile();
    let (POST, PP_FXAA) = (post as i32, fxaa as i32);
    let MIRROR_HQ = (mirror_hq > 0) as i32;
    let MIRROR_SMOKE = mirror_smoke as i32;
    let lines = [
        "[VIDEO]".to_string(),
        format!("WIDTH={width}"),
        format!("HEIGHT={height}"),
        "REFRESH=60".into(),
        "FULLSCREEN=0".into(),
        "VSYNC=0".into(),
        format!("AASAMPLES={AASAMPLES}"),
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
        format!("MODE={}", if triple { "TRIPLE" } else if oculus { "OCULUS" } else if vr { "OPENVR" } else { "DEFAULT" }),
        String::new(),
        "[ASSETTOCORSA]".into(),
        "HIDE_ARMS=0".into(),
        "HIDE_STEER=0".into(),
        "LOCK_STEER=0".into(),
        format!("WORLD_DETAIL={WORLD_DETAIL}"),
        String::new(),
        "[EFFECTS]".into(),
        format!("MOTION_BLUR={MOTION_BLUR}"),
        format!("RENDER_SMOKE_IN_MIRROR={MIRROR_SMOKE}"),
        format!("SMOKE={SMOKE}"),
        "FXAA=0".into(),
        String::new(),
        "[POST_PROCESS]".into(),
        format!("ENABLED={POST}"),
        "QUALITY=0".into(),
        "FILTER=default".into(),
        "GLARE=0".into(),
        "DOF=0".into(),
        "RAYS_OF_GOD=0".into(),
        "HEAT_SHIMMER=0".into(),
        format!("FXAA={PP_FXAA}"),
        String::new(),
        "[MIRROR]".into(),
        format!("HQ={MIRROR_HQ}"),
        format!("SIZE={MIRROR_SIZE}"),
        String::new(),
        "[CUBEMAP]".into(),
        format!("SIZE={CUBEMAP_SIZE}"),
        format!("FACES_PER_FRAME={CUBEMAP_FACES_PER_FRAME}"),
        format!("FARPLANE={CUBEMAP_FARPLANE}"),
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
    // CameraForwardYebis::readPPSetOptions reads the default filter
    copy_dir(&game.join("system/cfg/ppfilters"), &root.join("system/cfg/ppfilters"), false)?;
    copy_dir(&game.join("system/shaders/win"), &root.join("system/shaders/win"), false)?;
    copy_dir(&game.join("content/weather"), &root.join("content/weather"), true)?;
    // the crowds' textures
    for name in ["people_sit.dds", "people_stand.dds"] {
        let from = game.join("content/texture").join(name);
        if from.is_file() {
            std::fs::create_dir_all(root.join("content/texture")).map_err(|e| e.to_string())?;
            let to = root.join("content/texture").join(name);
            if !to.is_file() {
                std::fs::copy(&from, &to).map_err(|e| format!("{}: {e}", from.display()))?;
            }
        }
    }
    // the clouds' textures: the files of the folder itself
    copy_dir(&game.join("content/texture/clouds"), &root.join("content/texture/clouds"), false)?;
    std::fs::create_dir_all(root.join("cfg")).map_err(|e| e.to_string())?;
    let video = video_ini(args.width, args.height);
    let same = std::fs::read_to_string(root.join("cfg/video.ini")).is_ok_and(|old| old == video);
    if !same {
        std::fs::write(root.join("cfg/video.ini"), video).map_err(|e| e.to_string())?;
    }
    // … and where `INIReaderDocuments` looks for it
    let documents_cfg = root.join("documents/Assetto Corsa/cfg");
    std::fs::create_dir_all(&documents_cfg).map_err(|e| e.to_string())?;
    let video = video_ini(args.width, args.height);
    if !std::fs::read_to_string(documents_cfg.join("video.ini")).is_ok_and(|old| old == video) {
        std::fs::write(documents_cfg.join("video.ini"), video).map_err(|e| e.to_string())?;
    }
    if profile().triple && !std::fs::read_to_string(documents_cfg.join("triple_screen.ini")).is_ok_and(|old| old == TRIPLE_SCREEN_INI) {
        std::fs::write(documents_cfg.join("triple_screen.ini"), TRIPLE_SCREEN_INI).map_err(|e| e.to_string())?;
    }
    if profile().vr && !std::fs::read_to_string(documents_cfg.join("openvr.ini")).is_ok_and(|old| old == OPENVR_INI) {
        std::fs::write(documents_cfg.join("openvr.ini"), OPENVR_INI).map_err(|e| e.to_string())?;
    }
    if profile().oculus && !std::fs::read_to_string(documents_cfg.join("oculus.ini")).is_ok_and(|old| old == OCULUS_INI) {
        std::fs::write(documents_cfg.join("oculus.ini"), OCULUS_INI).map_err(|e| e.to_string())?;
    }
    let gameplay = format!("[VIRTUAL_MIRROR]\nACTIVE={}\n", profile().virtual_mirror as i32);
    if !std::fs::read_to_string(documents_cfg.join("gameplay.ini")).is_ok_and(|old| old == gameplay) {
        std::fs::write(documents_cfg.join("gameplay.ini"), &gameplay).map_err(|e| e.to_string())?;
    }
    if !std::fs::read_to_string(root.join("cfg/gameplay.ini")).is_ok_and(|old| old == gameplay) {
        std::fs::write(root.join("cfg/gameplay.ini"), &gameplay).map_err(|e| e.to_string())?;
    }
    // what the game's SkyBox constructor reads (the clouds of the weather)
    let race = format!("[WEATHER]\nNAME={}\n\n[LIGHTING]\nSUN_ANGLE={}\nTIME_MULT=1\nCLOUD_SPEED=0.2\n", args.weather, args.sun_angle);
    let same = std::fs::read_to_string(root.join("cfg/race.ini")).is_ok_and(|old| old == race);
    if !same {
        std::fs::write(root.join("cfg/race.ini"), race).map_err(|e| e.to_string())?;
    }
    Ok(())
}
