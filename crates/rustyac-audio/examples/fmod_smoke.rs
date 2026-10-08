// SPDX-License-Identifier: GPL-3.0-or-later

//! Smoke test of the FMOD binding: `fmod_smoke <AC folder> <out.wav> <event path> [block]`.

use rustyac_audio::fmod::raw::{self as f, Handle, In, Out, OutHandle, Output, Str};
use rustyac_audio::fmod::types::*;
use std::ffi::CString;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = std::path::PathBuf::from(&args[1]);
    let block: u32 = args.get(4).map(|s| s.parse().unwrap()).unwrap_or(800);
    f::load(&root, Output::WavNrt { file: args[2].clone().into(), rate: 48000, block }).unwrap();
    rustyac_audio::fmod::log::start(Some(std::path::Path::new(&format!("{}.log", args[2])))).unwrap();
    let guids = std::fs::read_to_string(root.join("content/sfx/GUIDs.txt")).unwrap();
    let line = guids.lines().find(|l| l.ends_with(&args[3])).expect("event not in GUIDs.txt");
    let hex: String = line[1..37].chars().filter(|c| *c != '-').collect();
    let b: Vec<u8> = (0..16).map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap()).collect();
    let guid = Guid {
        data1: u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
        data2: u16::from_be_bytes([b[4], b[5]]),
        data3: u16::from_be_bytes([b[6], b[7]]),
        data4: [b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]],
    };
    unsafe {
        let mut studio = std::ptr::null_mut();
        println!("create {}", f::studio_create(OutHandle(&mut studio), FMOD_VERSION));
        let studio = Handle(studio);
        let mut low = std::ptr::null_mut();
        f::studio_get_low_level_system(studio, OutHandle(&mut low));
        let mut version = 0u32;
        f::system_get_version(Handle(low), Out(&mut version));
        println!("version {version:#x}");
        println!("init {}", f::studio_initialize(studio, 256, 0, 0, std::ptr::null_mut()));
        let bank = CString::new(root.join("content/sfx/common.bank").to_string_lossy().into_owned()).unwrap();
        let mut b = std::ptr::null_mut();
        println!("bank {}", f::studio_load_bank_file(studio, Str(bank.as_ptr()), 0, OutHandle(&mut b)));
        let mut d = std::ptr::null_mut();
        println!("event {}", f::studio_get_event_by_id(studio, In(&guid), OutHandle(&mut d)));
        let mut i = std::ptr::null_mut();
        println!("instance {}", f::description_create_instance(Handle(d), OutHandle(&mut i)));
        println!("start {}", f::event_start(Handle(i)));
        let t = std::time::Instant::now();
        for _ in 0..180 {
            f::studio_update(studio);
        }
        println!("180 updates in {:?}", t.elapsed());
        println!("release {}", f::studio_release(studio));
    }
    println!("{:?}", rustyac_audio::fmod::log::finish().map(|s| (s.lines, s.hash)));
}
