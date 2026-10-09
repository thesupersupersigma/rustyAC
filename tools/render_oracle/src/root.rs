// SPDX-License-Identifier: GPL-3.0-or-later

//! The oracle's scratch game folder. The game's code builds relative paths (`system/cfg/…`,
//! `system/shaders/win/…`, `content/weather/…`, `cfg/video.ini`), so the oracle runs with its
//! current directory in a folder of its own that holds copies of those small files, and its
//! own `cfg/video.ini`: the Task 20 profile. The game's folder itself is only ever read; the
//! big files (kn5 models) are opened there through absolute paths.

use std::path::Path;

use crate::Args;

/// `cfg/video.ini` of the proof: no post-processing, no motion blur, no MSAA, no mirror, no
/// smoke, a cube map that renders no faces, shadows on at a fixed size.
pub fn video_ini(width: u32, height: u32) -> String {
    format!(
        "[VIDEO]\nWIDTH={width}\nHEIGHT={height}\nREFRESH=60\nFULLSCREEN=0\nVSYNC=0\nAASAMPLES=1\nAAQUALITY=0\nANISOTROPIC=8\nSHADOW_MAP_SIZE=2048\nFPS_CAP_MS=0\nINDEX=0\nDISABLE_LEGACY_HDR=1\n\n\
         [REFRESH]\nVALUE=60\n\n[CAMERA]\nMODE=DEFAULT\n\n[ASSETTOCORSA]\nHIDE_ARMS=0\nHIDE_STEER=0\nLOCK_STEER=0\nWORLD_DETAIL=5\n\n\
         [EFFECTS]\nMOTION_BLUR=0\nRENDER_SMOKE_IN_MIRROR=0\nSMOKE=0\nFXAA=0\n\n\
         [POST_PROCESS]\nENABLED=0\nQUALITY=0\nFILTER=default\nGLARE=0\nDOF=0\nRAYS_OF_GOD=0\nHEAT_SHIMMER=0\nFXAA=0\n\n\
         [MIRROR]\nHQ=0\nSIZE=512\n\n[CUBEMAP]\nSIZE=512\nFACES_PER_FRAME=0\nFARPLANE=500\n\n[SATURATION]\nLEVEL=100\n"
    )
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
        return Err(format!("{} looks like a game folder: the oracle's root has to be a scratch folder of its own", root.display()));
    }
    let game = &args.game;
    copy_dir(&game.join("system/cfg"), &root.join("system/cfg"), false)?;
    copy_dir(&game.join("system/shaders/win"), &root.join("system/shaders/win"), false)?;
    copy_dir(&game.join("content/weather"), &root.join("content/weather"), true)?;
    std::fs::create_dir_all(root.join("cfg")).map_err(|e| e.to_string())?;
    std::fs::write(root.join("cfg/video.ini"), video_ini(args.width, args.height)).map_err(|e| e.to_string())?;
    Ok(())
}
