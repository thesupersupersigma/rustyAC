// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 47c: the stand-in OpenVR runtime of `openvr_fake.rs` as an `openvr_api.dll`, for
//! `rustyac.exe` with video.ini `MODE=OPENVR` (it loads `<AC root>/system/x64/openvr_api.dll`).
//! With `OPENVR_STANDIN_OUT` set, every call goes to `calls.txt` there and the first
//! `OPENVR_STANDIN_FRAMES` (8) frames' eyes to `eye_<frame>_<eye>.png`.

#![allow(clippy::missing_safety_doc)]

use std::ffi::c_char;
use std::io::Write;
use std::sync::atomic::{AtomicU32, Ordering};

use rustyac_render::vr::Interface;

#[path = "openvr_fake.rs"]
#[allow(dead_code)]
mod openvr_fake;

static SUBMITTED: AtomicU32 = AtomicU32::new(0);

/// What the stand-in wrote down since the last time, into the folder (`VR_GetInitToken` runs
/// on every interface the port uses, so each frame).
fn flush() {
    let Some(dir) = std::env::var_os("OPENVR_STANDIN_OUT").map(std::path::PathBuf::from) else { return };
    let frames: u32 = std::env::var("OPENVR_STANDIN_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(8);
    let calls = openvr_fake::take_calls();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("calls.txt")) {
        for line in calls {
            let _ = writeln!(f, "{line}");
        }
    }
    for (eye, w, h, pixels) in openvr_fake::take_eyes() {
        let frame = SUBMITTED.fetch_add(1, Ordering::Relaxed) / 2;
        if frame >= frames {
            continue;
        }
        let Ok(file) = std::fs::File::create(dir.join(format!("eye_{frame:03}_{eye}.png"))) else { continue };
        let mut png = png::Encoder::new(std::io::BufWriter::new(file), w, h);
        png.set_color(png::ColorType::Rgba);
        if let Ok(mut out) = png.write_header() {
            let _ = out.write_image_data(&pixels);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn VR_InitInternal(error: *mut i32, kind: i32) -> u32 {
    (openvr_fake::api().init_internal)(error, kind)
}
#[no_mangle]
pub unsafe extern "C" fn VR_IsInterfaceVersionValid(name: *const c_char) -> bool {
    (openvr_fake::api().is_interface_version_valid)(name)
}
#[no_mangle]
pub unsafe extern "C" fn VR_GetGenericInterface(name: *const c_char, error: *mut i32) -> Interface {
    (openvr_fake::api().get_generic_interface)(name, error)
}
#[no_mangle]
pub unsafe extern "C" fn VR_GetInitToken() -> u32 {
    let token = (openvr_fake::api().get_init_token)();
    flush();
    token
}
#[no_mangle]
pub unsafe extern "C" fn VR_ShutdownInternal() {
    (openvr_fake::api().shutdown_internal)();
    flush();
}
#[no_mangle]
pub unsafe extern "C" fn VR_GetVRInitErrorAsEnglishDescription(error: i32) -> *const c_char {
    (openvr_fake::api().error_description)(error)
}
