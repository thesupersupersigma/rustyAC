// SPDX-License-Identifier: GPL-3.0-or-later

//! The game side of the replay bar's proofs: acs.exe's own `ReplayInterface` on the stand-in
//! `Sim` of the HUD scripts.
//!
//! `ReplayManager` is a block of memory with the members the bar reads; its transport
//! functions are replaced by stand-ins that write the call down and change those members by
//! the rules both sides share (`gui_seq_menu::replay_apply`; the manager itself is the proof
//! of Task 28c). `setCutInPerc` and `setCutOutPerc` are the game's own, without the pass over
//! the recorders that follows them. The camera manager is the HUD scripts' stand-in:
//! `ACCameraManager::setMode` is written down, the track's camera sets are the script's.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::ac::{rd, wr, wstring, write_wstring, Game};
use crate::gui_ac::{children, read_wstring};
use crate::gui_ac_hud::{acs, hud, jump, HudGame};
use crate::gui_seq_menu::{action, replay_apply, replay_set, script, ReplayModel};

pub const VA_REPLAY_INTERFACE_CTOR: usize = 0x1_4014_8300;
pub const VA_REPLAY_INTERFACE_VTABLE: usize = 0x1_404c_cdb0;
const VA_REPLAY_INTERFACE_RESET_CUTS: usize = 0x1_4014_f860;
const VA_CONTROL_SET_VISIBLE: usize = 0x1_4024_51a0;
const VA_GUI_ADD_CONTROL: usize = 0x1_4025_2e50;

static BAR: AtomicUsize = AtomicUsize::new(0);
static CAMERA_BEFORE: AtomicUsize = AtomicUsize::new(0);

fn bar() -> *mut u8 {
    BAR.load(Ordering::SeqCst) as *mut u8
}

unsafe fn manager() -> *mut u8 {
    rd(hud().expect("the HUD").sim, 0x1b0)
}

unsafe fn model(rm: *const u8) -> ReplayModel {
    ReplayModel { status: rd(rm, 0xbc), time_mult: rd(rm, 0x130), slow_mo_level: rd(rm, 0x138), current_frame: rd(rm, 0x174), recorded_frames: rd(rm, 0x14c), cut_in: rd(rm, 0x16c), cut_out: rd(rm, 0x168), has_loaded: rd::<u8>(rm, 0xb8) != 0 }
}

unsafe fn store(rm: *mut u8, m: &ReplayModel) {
    wr(rm, 0xbc, m.status);
    wr(rm, 0x130, m.time_mult);
    wr(rm, 0x138, m.slow_mo_level);
    wr(rm, 0x174, m.current_frame);
    wr(rm, 0x14c, m.recorded_frames);
    wr(rm, 0x16c, m.cut_in);
    wr(rm, 0x168, m.cut_out);
    wr(rm, 0xb8, m.has_loaded as u8);
}

unsafe fn ask(rm: *mut u8, what: &str, value: f32) {
    let mut m = model(rm);
    action(replay_apply(&mut m, what, value));
    store(rm, &m);
}

unsafe extern "C" fn rewind(this: *mut u8, _steps: i32, _b: bool) {
    ask(this, "rewind", 0.0);
}

unsafe extern "C" fn fast_forward(this: *mut u8, _steps: i32, _b: bool) {
    ask(this, "fastForward", 0.0);
}

unsafe extern "C" fn play(this: *mut u8) {
    ask(this, "play", 0.0);
}

unsafe extern "C" fn pause(this: *mut u8) {
    ask(this, "pause", 0.0);
}

unsafe extern "C" fn stop(this: *mut u8) {
    ask(this, "stop", 0.0);
}

unsafe extern "C" fn next_frame(this: *mut u8) {
    ask(this, "nextFrame", 0.0);
}

unsafe extern "C" fn slow_motion(this: *mut u8, level: f32) {
    ask(this, "slowMotion", level);
}

unsafe extern "C" fn stop_replay_mode(this: *mut u8) {
    ask(this, "stopReplayMode", 0.0);
}

unsafe extern "C" fn set_normalized_position(this: *mut u8, p: f32) {
    ask(this, "setNormalizedPosition", p);
}

unsafe extern "C" fn update_replay_recorders(_this: *mut u8, _frame: i32) {}

unsafe extern "C" fn get_normalized_lap_change(_this: *mut u8, _car: i32, out: *mut u8) {
    let marks = script().replay.lap_marks;
    if marks.is_empty() {
        return;
    }
    let block = acs().alloc(marks.len() * 4);
    for (i, mark) in marks.iter().enumerate() {
        wr(block, i * 4, *mark);
    }
    wr(out, 0, block);
    wr(out, 0x8, block.add(marks.len() * 4));
    wr(out, 0x10, block.add(marks.len() * 4));
}

unsafe fn empty_wstring(at: *mut u8) {
    std::ptr::write_bytes(at, 0, 0x20);
    wr(at, 0x18, 7usize);
}

unsafe extern "C" fn save(this: *mut u8, name: *mut u8, dir: i32) -> bool {
    let name = read_wstring(name);
    action(format!("replay save {name:?} {dir}"));
    write_wstring(acs(), this.add(0xc0), &name);
    true
}

unsafe extern "C" fn get_auto_file_name(_this: *mut u8, out: *mut u8) -> *mut u8 {
    empty_wstring(out);
    write_wstring(acs(), out, crate::gui_seq_menu::REPLAY_AUTO_NAME);
    out
}

unsafe extern "C" fn get_camera_manager(_sim: *mut u8) -> *mut u8 {
    if script().replay.camera_manager {
        hud().expect("the HUD").more.camera
    } else {
        std::ptr::null_mut()
    }
}

unsafe extern "C" fn set_current_camera_set(_this: *mut u8, set: i32) {
    action(format!("setCurrentCameraSet {set}"));
}

/// The script's values where the bar's code reads them itself.
pub unsafe fn sync(h: &HudGame) {
    if bar().is_null() {
        return;
    }
    let replay = script().replay;
    wr(h.more.camera, 0xdc, replay.camera_index);
    // CameraTrack's camera sets as the game's own code reads them (`getCameraTrackCount`
    // 0x1400cb660, `getCameraSetName` 0x14008f060: other classes share that code, so it is
    // not replaced): a vector of pointers at +0x78, each set's name at its +0x20
    let mut known = SETS.lock().unwrap();
    if *known != replay.camera_sets {
        let track: *mut u8 = rd(h.more.camera, 0xa0);
        let list = acs().alloc(replay.camera_sets.len().max(1) * 8);
        for (i, name) in replay.camera_sets.iter().enumerate() {
            let set = acs().alloc(0x40);
            empty_wstring(set.add(0x20));
            write_wstring(acs(), set.add(0x20), name);
            wr(list, i * 8, set);
        }
        wr(track, 0x78, list);
        wr(track, 0x80, list.add(replay.camera_sets.len() * 8));
        *known = replay.camera_sets;
    }
}

static SETS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// `Sim::cameraManager` is there while the GUI is drawn (`on`), as the bar's first frame
/// reads it; the HUD's proofs leave it empty between their own calls.
pub unsafe fn around_render(h: &HudGame, on: bool) {
    if bar().is_null() {
        return;
    }
    if on {
        CAMERA_BEFORE.store(rd::<usize>(h.sim, 0x188), Ordering::SeqCst);
        wr(h.sim, 0x188, h.more.camera);
    } else {
        wr(h.sim, 0x188, CAMERA_BEFORE.load(Ordering::SeqCst));
    }
}

/// Every listener of an event (a vector of 0x28-byte entries, the `std::function`'s
/// implementation the last word of each) hears `argument`.
pub unsafe fn fire(event: *const u8, argument: *const u8) {
    let (begin, end): (*const u8, *const u8) = (rd(event, 0), rd(event, 0x8));
    if begin.is_null() {
        return;
    }
    for i in 0..end.offset_from(begin) as usize / 0x28 {
        let implementation: *mut u8 = rd(begin.add(i * 0x28), 0x20);
        let table: *const usize = rd(implementation, 0);
        let call: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(*table.add(2));
        call(implementation, argument);
    }
}

/// A menu call of the script's that is the replay bar's (false: it is not).
pub unsafe fn call(game: &Game, gui: *mut u8, h: &HudGame, what: &str, number: f32, text: &str) -> bool {
    let a = &game.acs;
    let sim = h.sim;
    let rm: *mut u8 = rd(sim, 0x1b0);
    match what {
        "menu.replay" => {
            static DONE: AtomicBool = AtomicBool::new(false);
            if !DONE.swap(true, Ordering::SeqCst) {
                for (va, to) in [
                    (0x1_4015_9b40, rewind as *const () as usize),
                    (0x1_4015_6780, fast_forward as *const () as usize),
                    (0x1_4015_91f0, play as *const () as usize),
                    (0x1_4015_9070, pause as *const () as usize),
                    (0x1_4015_b6f0, stop as *const () as usize),
                    (0x1_4015_8900, next_frame as *const () as usize),
                    (0x1_4015_b3b0, slow_motion as *const () as usize),
                    (0x1_4015_b800, stop_replay_mode as *const () as usize),
                    (0x1_4015_b0b0, set_normalized_position as *const () as usize),
                    (0x1_4015_bc90, update_replay_recorders as *const () as usize),
                    (0x1_4015_70d0, get_normalized_lap_change as *const () as usize),
                    (0x1_4015_9d10, save as *const () as usize),
                    (0x1_4015_68b0, get_auto_file_name as *const () as usize),
                    (0x1_4006_2740, get_camera_manager as *const () as usize),
                    (0x1_400c_be10, set_current_camera_set as *const () as usize),
                ] {
                    jump(game, va, to);
                }
            }
            // the manager's members the bar reads; nobody else hears of the replay's status
            empty_wstring(rm.add(0xc0));
            store(rm, &ReplayModel::default());
            wr(sim, 0x80, rd::<usize>(sim, 0x78));
            // the camera manager's track cameras and the cameras that drive with the car
            wr(h.more.camera, 0xa0, a.alloc(0x100));
            wr(h.more.camera, 0xc0, a.alloc(0x100));
            let bar = a.alloc(0x330);
            let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(a.va(VA_REPLAY_INTERFACE_CTOR));
            ctor(bar, rm, sim);
            BAR.store(bar as usize, Ordering::SeqCst);
            // ReplayManager::ReplayManager: a top-level control of the GUI, hidden
            let add: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(a.va(VA_GUI_ADD_CONTROL));
            add(gui, bar);
            let set_visible: extern "C" fn(*mut u8, bool) = std::mem::transmute(a.va(VA_CONTROL_SET_VISIBLE));
            set_visible(bar, false);
        }
        // ReplayManager::startReplayMode / stopReplayMode, as far as the bar goes
        "menu.replayShow" => {
            sync(h);
            let set_visible: extern "C" fn(*mut u8, bool) = std::mem::transmute(a.va(VA_CONTROL_SET_VISIBLE));
            set_visible(bar(), number != 0.0);
            if number != 0.0 {
                let reset: extern "C" fn(*mut u8) = std::mem::transmute(a.va(VA_REPLAY_INTERFACE_RESET_CUTS));
                reset(bar());
            }
        }
        "menu.replaySet" => {
            let mut m = model(rm);
            replay_set(&mut m, text, number);
            store(rm, &m);
        }
        // ACCameraManager::setMode's last act: its listeners hear the camera's number
        "menu.cameraIndex" => {
            sync(h);
            let index = number as i32;
            fire(h.more.camera.add(0x58), &index as *const i32 as *const u8);
        }
        // ReplayManager::evDidFinishedSavingReplay
        "menu.replaySaved" => {
            let ok = number != 0.0;
            fire(rm.add(0xa0), &ok as *const bool as *const u8);
        }
        _ => return false,
    }
    let _ = wstring;
    true
}

/// The bar's own state in the dump.
pub unsafe fn extra(control: *const u8) -> String {
    let last: *mut u8 = rd(control, 0x1f0);
    let index = children(control).iter().position(|c| *c == last).map_or(-1, |i| i as i32);
    let names = (rd::<usize>(control, 0x238) - rd::<usize>(control, 0x230)) / 0x20;
    crate::gui_seq_menu::replay_extra(rd::<u8>(control, 0x178) != 0, rd(control, 0x180), rd(control, 0x184), index, names)
}

/// The manager's members, and the camera that drives with the car, in the dump.
pub unsafe fn dump(out: &mut String) {
    if bar().is_null() {
        return;
    }
    let h = hud().expect("the HUD");
    let drivable: *const u8 = rd(h.more.camera, 0xc0);
    out.push_str(&crate::gui_seq_menu::replay_line(&model(manager()), rd(drivable, 0x58)));
}
